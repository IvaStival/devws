# Slug bug

`slugify` in `slugs/slugify.py` drops the last word of a title: "Hello Big
World" becomes `hello-big` instead of `hello-big-world`. Fix it. This is the
benchmark's fixed scope: give it to the leader unchanged on every run.

- Change only `slugs/slugify.py`. Nothing else in the repo uses the bug.
- Words are separated by whitespace, lowercased and joined with `-`; an
  empty or blank title gives an empty slug. That part works today.

Done means `python3 -m unittest discover -s tests -t .` passes, including the
acceptance tests in `tests/acceptance/`, which judge the work and are not
changed by the fix.
