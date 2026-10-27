bcrypt / bcryptjs: async code awaits `hash()` / `compare()` / `genSalt()`; the `*Sync` variants block the event loop and are rejected in async functions.
