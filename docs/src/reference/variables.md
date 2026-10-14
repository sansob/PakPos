# Variables

Write `{{name}}` in any text field. Spaces inside the braces are ignored (`{{ name }}`).

## Resolution order

1. Runtime values (extractions during a run, `--var` on the CLI)
2. Active environment
3. Collection variables
4. Globals
5. Dynamic variables

Unknown variables are left as-is (`{{missing}}`) so you can spot them; the URL
bar marks them red and lists them below. Variables can reference other variables
(`base_url = {{scheme}}://{{host}}`), resolved up to 8 levels deep, which also
protects against self-references.

## Dynamic variables

`{{$guid}}`, `{{$randomUUID}}`, `{{$timestamp}}`, `{{$isoTimestamp}}`,
`{{$randomInt}}`, `{{$randomBoolean}}`, `{{$randomEmail}}`. They use the common
names, so imported collections keep working. A new value is produced
every time one is resolved.

## Secrets

Environment files are plain JSON. Do not commit real secrets: keep a
`staging.example.json` in git and pass real values with `--var token=$TOKEN` in
CI, or keep the secret in an environment that only lives in your local
workspace. Code snippets resolve variables, so they include secrets; share them with care.
