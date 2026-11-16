ADR: implement slugify() in place

Context
The repository holds slugify.py with an unimplemented slugify(text) and test_slugify.py
with the acceptance tests.

Decision
Implement slugify(text) in slugify.py using only the Python standard library:
normalise to NFKD and drop what has no ASCII form, lower-case, replace every run of
characters other than a-z and 0-9 with one hyphen, and strip hyphens from both ends.
Raise TypeError for a value that is not a string.

Constraints
Change slugify.py only. Do not change, rename, delete or add tests: test_slugify.py is
the approved baseline. Add no file, no dependency and no network access.

Validation
The host runs /usr/bin/python3 -m unittest -q on the sealed candidate with the network
denied. A statement by the agent that tests passed is not a result.
