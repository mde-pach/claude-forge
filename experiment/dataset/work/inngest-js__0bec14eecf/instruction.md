# Connect: lease extensions must not be sent over a draining connection

## Problem

When a worker using `connect` is processing an executor request, it periodically sends `WORKER_REQUEST_EXTEND_LEASE` messages to keep the request's lease alive. These extensions are sent over the currently active WebSocket connection. If no connection is active, they fall back to the connection the request originally arrived on.

This fallback is wrong when the gateway is draining a connection (it has sent a "gateway closing" message and a replacement connection is still handshaking). The following sequence can occur:

1. During the drain window, the worker sends a lease extension over the draining socket.
2. The server processes the extension and issues a new lease ID.
3. The ACK is lost when the old socket is closed in favour of the replacement.
4. The next extension uses the superseded lease ID, so the lease appears to have been lost.

The same problem occurs after chained reconnects. Suppose a request arrived on connection A, the worker reconnected to B after an ordinary error, and B then starts draining. While the replacement C is handshaking, the worker must not fall back to A (or B) for lease extensions.

## Required behaviour

On each lease-extension tick for an in-flight request:

- **Active connection exists:** if there is a currently active connection, use it.
- **No active connection, nothing draining:** if there is no active connection and no connection is draining (an ordinary reconnect), fall back to the request's original connection, as before.
- **No active connection, a connection is draining:** do **not** send the extension on any connection. That means not the draining one, not the original one, and not any older one. Skip the tick.
- **Chosen connection not open:** if no connection is chosen, or the chosen connection's WebSocket is not `OPEN`, skip the tick. Log a warning with the message `"Cannot extend lease, no open WebSocket available"`.

## Recovery once the replacement is ready

Once the replacement connection completes its handshake and becomes active:

- Subsequent ticks must send lease extensions over that new connection.
- They must use the request's current lease ID. For example, if no extension was ACKed during the drain, this is still the original lease ID received with the request.
- The request continues normally until its execution completes.

Skipped ticks must not stop the lease-extension loop. Ticks keep firing at the normal interval for the lifetime of the request.
