---
"@omnidotdev/run-cli": patch
---

Update the Creator app image so `SELF_HOST_MODE` takes effect in the self-hosted build. The previous image baked `VITE_AUTH_BASE_URL`, which forced authentication on and 307-redirected every authenticated route; the new build honors single-owner self-host mode.
