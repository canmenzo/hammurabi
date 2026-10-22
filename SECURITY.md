# Security policy

## Supported versions

Only the latest release gets security fixes.

## Reporting a vulnerability

Please do not open a public issue for security problems. Report them privately through [GitHub security advisories](https://github.com/canmenzo/hammurabi/security/advisories/new) instead.

Include what you found, how to reproduce it, and which version and OS you tested. You should get a reply within a week. Once a fix is released, the advisory is published with credit to you unless you prefer to stay anonymous.

## Scope

Hammurabi runs locally. Its only network traffic is model downloads from Hugging Face, the update check against GitHub releases, and, if you configure one, requests to your own local LLM server. Reports about any of these, about the model download and loading path, or about anything that could make audio or captions leave the computer are especially welcome.
