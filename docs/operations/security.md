# Security notes

See also root [SECURITY.md](https://github.com/joe-broadhead/aurum/blob/main/SECURITY.md).

## Default path

- No API key  
- Network only for **explicit model download** when a ggml file is missing  
- No analytics / telemetry  

## Remote path

- Only when `--provider openrouter`  
- Key via env preferred; never logged  
- Uploads compressed and size-capped  

## Integrity

- Model magic verified (fail closed)  
- Pinned SHA-256 for selected models (`tiny`, `tiny-q5_1`)  
- Cross-process download lock  

## Reporting

Use GitHub Security Advisories.
