Stripe on Workers: webhooks are verified with `await stripe.webhooks.constructEventAsync(body, signature, secret)` (SubtleCrypto is async only; `constructEvent` throws there).
