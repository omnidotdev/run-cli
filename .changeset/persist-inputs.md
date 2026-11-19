---
"@omnidotdev/run-cli": minor
---

Typed inputs are now remembered per stack. A value you set through `RUN_INPUT_<KEY>` or answer at the prompt is saved with the stack, so `run update`, a later `run up`, and the dashboard reuse it instead of prompting again or falling back to the default. Set `RUN_INPUT_<KEY>` again to change it. The stack's generated settings file, which holds these values next to its secrets, is now readable by its owner only.
