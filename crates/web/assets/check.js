// The connection check (SPEC-0325 behaviour 16): authenticates and says hello over the session
// protocol's frames. Results are written to the page only; the credential goes nowhere else.
const out = document.getElementById("out");
const log = (line) => { out.textContent += line + "\n"; };

function frame(json) {
  const text = new TextEncoder().encode(json);
  const bytes = new Uint8Array(9 + text.length);
  const view = new DataView(bytes.buffer);
  view.setUint32(0, 5 + text.length);
  bytes[4] = 1;
  view.setUint32(5, 0);
  bytes.set(text, 9);
  return bytes;
}

document.getElementById("form").addEventListener("submit", (event) => {
  event.preventDefault();
  out.textContent = "";
  const started = performance.now();
  const ws = new WebSocket("wss://" + location.host + "/");
  ws.binaryType = "arraybuffer";
  let authed = false;
  ws.onopen = () => {
    log("TLS and WebSocket open after " + Math.round(performance.now() - started) + " ms");
    ws.send(JSON.stringify({ credential: document.getElementById("credential").value }));
  };
  ws.onmessage = (message) => {
    if (!authed) {
      authed = true;
      log("authenticated: " + message.data);
      ws.send(frame(JSON.stringify({
        jsonrpc: "2.0", id: 1, method: "session.hello",
        params: {
          protocol: "idecraft-session", majors: [1], build: "check", client: "check",
          project: document.getElementById("project").value, capabilities: [],
        },
      })));
      return;
    }
    const body = new Uint8Array(message.data).slice(9);
    log("session.hello: " + new TextDecoder().decode(body).slice(0, 400));
    log("round trip after " + Math.round(performance.now() - started) + " ms");
    ws.close();
  };
  ws.onclose = (close) => log("closed: " + close.code + " " + close.reason);
  ws.onerror = () => log("connection failed");
});
