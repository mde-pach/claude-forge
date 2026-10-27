The msw server of the tests starts with `server.listen({ onUnhandledRequest: "error" })`, so a request no handler covers fails the test instead of reaching the network.
