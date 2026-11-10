#!/usr/bin/env perl
# Emits only usage metadata from NUL-separated transcript paths on stdin.
# Compact per-file records preserve cross-file deduplication and time rules.
use strict;
use warnings;
use Cwd qw(abs_path);
use Digest::SHA qw(sha256_hex);
use File::Temp qw(tempfile);
use JSON::PP;
use Time::HiRes ();

my $json = JSON::PP->new->utf8->canonical;
my $cache_directory = ($ENV{TMPDIR} || '/tmp') . '/devws-tokens-' . $<;
my $can_cache = !$ENV{DEVWS_TOKEN_CACHE_DISABLE};
if ($can_cache) {
    mkdir $cache_directory, 0700 unless -e $cache_directory;
    my @metadata = lstat $cache_directory;
    $can_cache = @metadata && -d _ && !-l _ && $metadata[4] == $< && ($metadata[2] & 0777) == 0700;
}

sub fingerprint {
    my ($path) = @_;
    my @metadata = Time::HiRes::stat($path);
    return undef unless @metadata;
    return join ':', @metadata[0, 1, 7], map { sprintf '%.9f', $_ } @metadata[9, 10];
}

sub fields {
    my ($source, @names) = @_;
    return {} unless ref $source eq 'HASH';
    return {map { exists $source->{$_} ? ($_ => $source->{$_}) : () } @names};
}

sub usage_fields {
    return fields($_[0], qw(input_tokens output_tokens total_tokens cached_input_tokens
        cache_write_input_tokens cache_creation_input_tokens cache_read_input_tokens));
}

sub compact_record {
    my ($record) = @_;
    return undef unless ref $record eq 'HASH';
    my $type = $record->{type} || '';
    my $result = fields($record, qw(type timestamp));
    if ($type eq 'assistant') {
        my $message = $record->{message};
        return undef unless ref $message eq 'HASH' && ($message->{model}
            || (ref $message->{usage} eq 'HASH' && $message->{id}));
        my $usage = usage_fields($message->{usage});
        $usage->{cache_creation} = fields($message->{usage}{cache_creation},
            qw(ephemeral_5m_input_tokens ephemeral_1h_input_tokens)) if ref $message->{usage} eq 'HASH' && exists $message->{usage}{cache_creation};
        $result->{message} = fields($message, qw(id model));
        $result->{message}{usage} = $usage if ref $message->{usage} eq 'HASH';
        return $result;
    }
    my $payload = $record->{payload};
    return undef unless ref $payload eq 'HASH';
    if ($type eq 'turn_context') {
        $result->{payload} = fields($payload, qw(turn_id model));
    } elsif ($type eq 'token_usage_record') {
        $result->{payload} = fields($payload, qw(turn_id));
        $result->{payload}{usage} = usage_fields($payload->{usage});
    } elsif ($type eq 'event_msg' && ($payload->{type} || '') eq 'token_count') {
        my $filtered = {type => 'token_count', info => undef};
        if (ref $payload->{info} eq 'HASH') {
            $filtered->{info} = fields($payload->{info}, qw(model_context_window));
            for my $key (qw(total_token_usage last_token_usage)) {
                $filtered->{info}{$key} = usage_fields($payload->{info}{$key});
            }
        }
        $result->{payload} = $filtered;
    } elsif (ref $payload->{rate_limits} eq 'HASH') {
        $result->{payload} = {};
    } else {
        return undef;
    }
    if (ref $payload->{rate_limits} eq 'HASH') {
        $result->{payload}{rate_limits} = {};
        for my $window (qw(primary secondary)) {
            $result->{payload}{rate_limits}{$window} = ref $payload->{rate_limits}{$window} eq 'HASH'
                ? fields($payload->{rate_limits}{$window}, qw(used_percent resets_at window_minutes)) : undef;
        }
    }
    return $result;
}

sub parse_records {
    my ($path) = @_;
    open my $source, '<:raw', $path or return undef;
    my ($records, $bytes) = ('', 0);
    while (my $line = <$source>) {
        $bytes += length $line;
        # Avoid decoding tool output and prompts, which make up most logs.
        next unless $line =~ /"(?:model|output_tokens|turn_context|token_usage_record|token_count|rate_limits)"/;
        my $record = eval { $json->decode($line) };
        next if $@;  # An incomplete JSONL tail is retried when the file changes.
        my $compact = compact_record($record);
        $records .= $json->encode($compact) . "\n" if $compact;
    }
    close $source;
    if (my $metrics = $ENV{DEVWS_TOKEN_CACHE_METRICS}) {
        if (open my $output, '>>', $metrics) {
            print {$output} "parse $bytes\n";
            close $output;
        }
    }
    return $records;
}

sub records_for {
    my ($path) = @_;
    $path = abs_path($path);
    return '' unless defined $path && -f $path;
    my $before = fingerprint($path);
    return '' unless defined $before;
    # Bump the schema when the retained fields or parser semantics change.
    my $entry = $cache_directory . '/' . sha256_hex("v3\0$path") . '.json';
    if ($can_cache && !-l $entry && open my $cached, '<:raw', $entry) {
        my $contents = do { local $/; <$cached> };
        close $cached;
        my $stored = eval { $json->decode($contents) };
        if (!$@ && ref $stored eq 'HASH' && ($stored->{fingerprint} || '') eq $before
            && defined $stored->{records} && !ref $stored->{records}) {
            return $stored->{records};
        }
    }
    my $records = parse_records($path);
    return '' unless defined $records;
    my $after = fingerprint($path);
    if ($can_cache && defined $after && $after eq $before) {
        # Readers see either complete generation; competing writers are safe.
        my ($temporary, $filename) = eval { tempfile('write-XXXXXXXX', DIR => $cache_directory, UNLINK => 0) };
        if ($temporary) {
            binmode $temporary;
            my $written = print {$temporary} $json->encode({fingerprint => $before, records => $records});
            my $closed = close $temporary;
            unlink $filename unless $written && $closed && rename $filename, $entry;
        }
    }
    return $records;
}

# Bound abandoned account/file entries without a separate background process.
if ($can_cache) {
    my $marker = "$cache_directory/.pruned";
    if (!-e $marker || time - (stat $marker)[9] > 3600) {
        if (opendir my $directory, $cache_directory) {
            while (my $name = readdir $directory) {
                next unless $name =~ /\A[0-9a-f]{64}\.json\z/;
                my $entry = "$cache_directory/$name";
                my @metadata = lstat $entry;
                unlink $entry if @metadata && -f _ && time - $metadata[9] > 32 * 86400;
            }
            closedir $directory;
        }
        if (open my $output, '>', $marker) { close $output; }
    }
}

binmode STDOUT;
while (defined(my $path = do { local $/ = "\0"; <STDIN> })) {
    $path =~ s/\0$//;
    print records_for($path);
}
