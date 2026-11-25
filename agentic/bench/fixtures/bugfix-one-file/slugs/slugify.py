"""URL slugs for note titles."""


def slugify(title):
    """The title's words, lowercased and joined with dashes."""
    words = title.lower().split()
    return "-".join(words[:-1])
