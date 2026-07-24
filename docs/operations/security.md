# Security notes

See also root [SECURITY.md](https://github.com/joe-broadhead/aurum/blob/main/SECURITY.md).

## Default path

- No API key  
- Network only for **explicit model download** when a ggml file is missing  
- No analytics / telemetry  

## Remote path

- Only when `--provider openrouter` or cleanup provider `openrouter`  
- Key via environment preferred; redacted in `Debug`  
- Uploads compressed and size-capped  

## Integrity

- Model magic verified (fail closed)  
- Pinned SHA-256 for common models (`tiny`, `tiny-q5_1`, `tiny.en-q5_1`, `base`, `base-q5_1`, `base.en-q5_1`, `small-q5_1`)  
- Download stream size cap  
- Cross-process download lock  
- O_EXCL temp files for uploads  

## Reporting

Use GitHub Security Advisories.
