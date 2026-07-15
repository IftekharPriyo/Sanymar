# Database migration reference

- Every schema change is a new ordered SQL migration.
- Do not edit a migration after shared application; ask before destructive changes.
- Keep tokens out of SQLite and avoid unnecessary raw provider payloads.
- Justify indexes with an actual query path.
- Exercise repository behavior against real migrations in tests.
