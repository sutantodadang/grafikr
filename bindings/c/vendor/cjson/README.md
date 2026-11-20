cJSON 1.7.19 is vendored from DaveGamble/cJSON tag `v1.7.19`
(commit `c859b25`). The source archive SHA-256 is
`7fa616e3046edfa7a28a32d5f9eacfd23f92900fe1f8ccd988c1662f30454562`.

Grafikr uses only `cJSON.c` and `cJSON.h`; the MIT license is in `LICENSE`.
The local parser patch limits JSON whitespace to space, tab, LF, and CR, as
required by the JSON grammar.
