# Zode SDK Contract

`basic_run.json` documents the shared request sequence every SDK should be able
to send over stdio for the currently implemented server surface.

The current server returns a registry turn for `turn/start`; it does not stream
model output yet.

## Use

SDK tests can use this contract as a common flow:

1. Send `initialize`.
2. Send `thread/start`.
3. Substitute the returned thread id into `turn/start`.
4. Send `command/exec`.

The flow intentionally avoids account, marketplace, remote-control, Realtime,
websocket, and background process APIs because they are not part of the current
zode-backed surface.
