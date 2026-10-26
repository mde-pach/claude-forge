Upstash Ratelimit: `limit()` never throws; its result is checked (`const { success } = await ratelimit.limit(id)`), never discarded.
