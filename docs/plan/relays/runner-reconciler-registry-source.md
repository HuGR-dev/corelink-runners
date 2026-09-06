# Runner reconciler registry relay

The relay targets `corelink-server` source commit
`c0a4466d930073d05ebedc3ac8b8d9cc8474b68f` with source tree
`79b51bfd163fb41e31fb9ab52105110a06e6d4cf`.

The applicable patch is `runner-reconciler-registry.patch`. It adds the
authoritative registry producer, its focused tests, and the Worker import,
route match, and handler dispatch. Validation was run with `git apply --check`
against a fresh archive of that source commit.
