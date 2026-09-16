# Schema scope

These JSON Schema 2020-12 documents describe the alpha configuration and task files.
The Go decoder additionally rejects duplicate/case-aliased keys and null required
fields; Go validation enforces duration, argv bytes, path safety and duplicate IDs.
JSON Schema alone cannot enforce every runtime constraint. `internal/model` defines
record envelopes. State/report compatibility is v1alpha1 and not yet a v1 promise.
