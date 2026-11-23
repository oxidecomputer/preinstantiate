# Agent instructions for preinstantiate

## Tests

- Always write snapshot assertions as `expectorate::assert_contents` with the
  snapshot's full path, for example
  `expectorate::assert_contents("tests/output/module_tree.txt", &contents)`.
  Don't wrap it in a helper that builds the path, so that each test shows
  exactly which file it checks.
