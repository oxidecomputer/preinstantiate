# Agent instructions for preinstantiate

## Tests

- Always write snapshot assertions as `expectorate::assert_contents` with the
  snapshot's full path, for example
  `expectorate::assert_contents("tests/output/module_tree.txt", &contents)`.
  Don't wrap it in a helper that builds the path, so that each test shows
  exactly which file it checks.

## Naming and prose

- Never use "skip" as a noun: not in error messages, docs, comments, commit
  messages, or identifiers (`UnusedSkip`, `for skip in ...`). Name the thing
  itself instead, such as "a `skip_method` call" or "a skipped method". In
  error messages, quote the `skip_method(...)` call the user wrote.
