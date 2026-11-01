# Planner fixtures

These are inputs and handwritten argv expectations for `planner.v1`, not a JSON
Schema or another implementation of the planner. The runner renders `@ENGINE@`,
`@SOURCE@` and `@DESTINATION@` using fresh absolute temporary paths. No fixture
depends on a particular rsync installation or ignored role documents.

The two task templates and `expected-argv.json` define Recursive and Archive
independently. Invalid JSON, duplicate keys and boundary cases are generated from
these templates without invoking kernel code. Object key order and JSON whitespace
are not expected to round-trip; field values, raw paths and argv order are.

`engine-marker.sh` is copied into the temporary fixture, made executable there,
and supplied as an engine path. It must never run: execution would create a
`.executed` marker and fail the fixture integrity check. The runner never launches
this asset itself. Its repository copy need not be executable.

Generated data includes regular/empty/binary files, Unicode and shell-literal
directory names, independent component prefixes, top-level aliases, nested and
dangling symlinks. Symlinks inside the source are not followed by the fixture
manifest. Permission tests first confirm the current identity actually receives
PermissionError; root or other environments that bypass permissions are reported
as unverified, never as passing those assertions.

The runner compares content hashes, entries, link targets, modes and mtime around
every probe call. atime is excluded because read-only access can update it.
Harness-controlled permission changes and directory removal happen before the
corresponding per-call baseline. No business data or real transfer is used.
