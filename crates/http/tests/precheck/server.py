#!/usr/bin/env python3
"""Local test server for the request files in this folder (SPEC-0046, SPEC-0096, SPEC-0102).

Python standard library only. Listens on 127.0.0.1 and nowhere else; the port is chosen by the
system unless --port is given. Prints one line `listening on http://127.0.0.1:<port>` when ready
and ends with its parent's signal (Ctrl+C, SIGTERM).

    python3 -I server.py [--port N] [--tls-cert FILE --tls-key FILE]

Endpoints (every method unless noted):

    /echo, /echo/...        JSON of what arrived: method, path, query, headers, body
    /redirect/301|302|303|307|308[?to=/path]   that status with Location (default /echo)
    /redirect/loop          302 to itself
    /redirect/relative      302 with a relative Location (`../echo?from=relative`)
    /redirect/other-host[/CODE]   to the same port under the other loopback name
                            (127.0.0.1 -> localhost, anything else -> 127.0.0.1), path /echo
    /redirect/scheme-relative     302 with `//<other loopback name>:port/echo`
    /redirect/no-location   302 without a Location header
    /redirect/file          302 with Location `file:///etc/hosts` (must never be followed)
    /cookies/set            204 with two Set-Cookie headers
    /cookies/show           JSON with the Cookie header that arrived
    /chunked                chunked transfer coding, three chunks and a trailer
    /gzip                   gzip content coding when the request accepts it, else plain JSON
    /gzip/forced            gzip content coding whatever the request accepts
    /slow?seconds=S         answers after S seconds (default 2)
    /large?bytes=N          N bytes of text (default 1048576)
    /binary                 256 bytes 0..255 as application/octet-stream
    /type/wrong             JSON text labelled text/plain
    /type/missing           JSON text without Content-Type
    /type/broken-json       application/json with a body that is not JSON
    /type/latin1            text/plain; charset=iso-8859-1 with bytes that are not UTF-8
    /status/CODE            that status; 204 and 304 without body
    /close-delimited        body ended by closing the connection, no length
    /interim                a 100 Continue before the 200
    /basic-auth             401 with WWW-Authenticate unless an Authorization header arrived
"""

import argparse
import base64
import gzip
import json
import ssl
import sys
import time
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from urllib.parse import parse_qs, urlsplit

HOST = "127.0.0.1"


class Handler(BaseHTTPRequestHandler):
    protocol_version = "HTTP/1.1"
    server_version = "ferris-idea-test-server"
    sys_version = ""

    def log_message(self, fmt, *args):  # quiet; nothing with headers is ever written
        pass

    # -- helpers ---------------------------------------------------------------------------

    def read_body(self):
        if "chunked" in self.headers.get("Transfer-Encoding", "").lower():
            data = b""
            while True:
                size = int(self.rfile.readline().split(b";")[0].strip() or b"0", 16)
                if size == 0:
                    while self.rfile.readline().strip():
                        pass
                    return data
                data += self.rfile.read(size)
                self.rfile.readline()
        length = int(self.headers.get("Content-Length") or 0)
        return self.rfile.read(length) if length else b""

    def send(self, status, body=b"", content_type="application/json", headers=(), length=True):
        self.send_response(status)
        if content_type:
            self.send_header("Content-Type", content_type)
        for name, value in headers:
            self.send_header(name, value)
        if length:
            self.send_header("Content-Length", str(len(body)))
        self.send_header("Connection", "close")
        self.end_headers()
        if self.command != "HEAD":
            self.wfile.write(body)
        self.close_connection = True

    def port(self):
        return self.server.server_address[1]

    def scheme(self):
        return "https" if self.server.tls else "http"

    def other_host(self):
        host = self.headers.get("Host", "").rsplit(":", 1)[0]
        return "localhost" if host == "127.0.0.1" else "127.0.0.1"

    def echo(self, url, query, body):
        try:
            text = body.decode("utf-8")
            encoded = None
        except UnicodeDecodeError:
            text = None
            encoded = base64.b64encode(body).decode("ascii")
        return json.dumps(
            {
                "method": self.command,
                "path": url.path,
                "query": query,
                "raw_query": url.query,
                "target": self.path,
                "headers": [[k, v] for k, v in self.headers.items()],
                "body": text,
                "body_base64": encoded,
                "body_length": len(body),
            },
            indent=None,
        ).encode("utf-8")

    # -- routing ---------------------------------------------------------------------------

    def handle_any(self):
        url = urlsplit(self.path)
        query = parse_qs(url.query, keep_blank_values=True)
        body = self.read_body()
        path = url.path
        parts = [p for p in path.split("/") if p]

        if path == "/echo" or path.startswith("/echo/"):
            return self.send(200, self.echo(url, query, body))

        if parts[:1] == ["redirect"] and len(parts) >= 2:
            kind = parts[1]
            base = "%s://%s:%d" % (self.scheme(), self.other_host(), self.port())
            if kind in ("301", "302", "303", "307", "308"):
                to = query.get("to", ["/echo"])[0]
                return self.send(int(kind), b"", None, [("Location", to)])
            if kind == "loop":
                return self.send(302, b"", None, [("Location", "/redirect/loop")])
            if kind == "relative":
                return self.send(302, b"", None, [("Location", "../echo?from=relative")])
            if kind == "other-host":
                code = int(parts[2]) if len(parts) > 2 else 302
                return self.send(code, b"", None, [("Location", base + "/echo")])
            if kind == "scheme-relative":
                location = "//%s:%d/echo" % (self.other_host(), self.port())
                return self.send(302, b"", None, [("Location", location)])
            if kind == "no-location":
                return self.send(302, b"no location", "text/plain")
            if kind == "file":
                return self.send(302, b"", None, [("Location", "file:///etc/hosts")])

        if path == "/cookies/set":
            return self.send(
                204,
                b"",
                None,
                [
                    ("Set-Cookie", "session=test-session-1; Path=/; HttpOnly"),
                    ("Set-Cookie", "theme=dark; Path=/cookies"),
                ],
            )
        if path == "/cookies/show":
            cookie = self.headers.get("Cookie")
            return self.send(200, json.dumps({"cookie": cookie}).encode("utf-8"))

        if path == "/chunked":
            self.send_response(200)
            self.send_header("Content-Type", "application/json")
            self.send_header("Transfer-Encoding", "chunked")
            self.send_header("Trailer", "X-Checksum")
            self.send_header("Connection", "close")
            self.end_headers()
            if self.command != "HEAD":
                for piece in (b'{"chunks": ', b"3, ", b'"ok": true}'):
                    self.wfile.write(b"%x;note=1\r\n%s\r\n" % (len(piece), piece))
                    self.wfile.flush()
                self.wfile.write(b"0\r\nX-Checksum: none\r\n\r\n")
            self.close_connection = True
            return None

        if path in ("/gzip", "/gzip/forced"):
            plain = json.dumps({"gzipped": True, "text": "x" * 200}).encode("utf-8")
            accepts = "gzip" in self.headers.get("Accept-Encoding", "").lower()
            if path == "/gzip/forced" or accepts:
                packed = gzip.compress(plain)
                return self.send(
                    200, packed, "application/json", [("Content-Encoding", "gzip"), ("Vary", "Accept-Encoding")]
                )
            return self.send(200, plain, "application/json", [("Vary", "Accept-Encoding")])

        if path == "/slow":
            time.sleep(float(query.get("seconds", ["2"])[0]))
            return self.send(200, b'{"slow": true}')

        if path == "/large":
            size = int(query.get("bytes", ["1048576"])[0])
            line = b"0123456789abcdef" * 4 + b"\n"
            data = (line * (size // len(line) + 1))[:size]
            return self.send(200, data, "text/plain; charset=utf-8")

        if path == "/binary":
            return self.send(200, bytes(range(256)), "application/octet-stream")

        if parts[:1] == ["type"] and len(parts) == 2:
            kind = parts[1]
            if kind == "wrong":
                return self.send(200, b'{"labelled": "text/plain"}', "text/plain")
            if kind == "missing":
                return self.send(200, b'{"labelled": null}', None)
            if kind == "broken-json":
                return self.send(200, b'{"broken": ', "application/json")
            if kind == "latin1":
                return self.send(200, "Grüße".encode("iso-8859-1"), "text/plain; charset=iso-8859-1")

        if parts[:1] == ["status"] and len(parts) == 2 and parts[1].isdigit():
            code = int(parts[1])
            if code in (204, 304):
                self.send_response(code)
                self.send_header("Connection", "close")
                self.end_headers()
                self.close_connection = True
                return None
            return self.send(code, json.dumps({"status": code}).encode("utf-8"))

        if path == "/close-delimited":
            self.send_response(200)
            self.send_header("Content-Type", "text/plain")
            self.send_header("Connection", "close")
            self.end_headers()
            if self.command != "HEAD":
                self.wfile.write(b"ends when the connection closes")
            self.close_connection = True
            return None

        if path == "/interim":
            self.wfile.write(b"HTTP/1.1 100 Continue\r\n\r\n")
            self.wfile.flush()
            return self.send(200, b'{"after": "100"}')

        if path == "/basic-auth":
            if self.headers.get("Authorization"):
                return self.send(200, b'{"authorised": true}')
            return self.send(401, b'{"authorised": false}', "application/json", [("WWW-Authenticate", 'Basic realm="test"')])

        return self.send(404, json.dumps({"error": "no such endpoint", "path": path}).encode("utf-8"))

    do_GET = do_POST = do_PUT = do_PATCH = do_DELETE = do_HEAD = do_OPTIONS = do_TRACE = handle_any


def main():
    parser = argparse.ArgumentParser(description="Local HTTP test server on 127.0.0.1")
    parser.add_argument("--port", type=int, default=0, help="0 lets the system choose (default)")
    parser.add_argument("--tls-cert", help="certificate file; with --tls-key the server speaks TLS")
    parser.add_argument("--tls-key")
    args = parser.parse_args()

    server = ThreadingHTTPServer((HOST, args.port), Handler)
    server.daemon_threads = True
    server.tls = bool(args.tls_cert and args.tls_key)
    if server.tls:
        context = ssl.SSLContext(ssl.PROTOCOL_TLS_SERVER)
        context.load_cert_chain(args.tls_cert, args.tls_key)
        server.socket = context.wrap_socket(server.socket, server_side=True)
    scheme = "https" if server.tls else "http"
    print("listening on %s://%s:%d" % (scheme, HOST, server.server_address[1]), flush=True)
    try:
        server.serve_forever()
    except KeyboardInterrupt:
        pass
    finally:
        server.server_close()
    return 0


if __name__ == "__main__":
    sys.exit(main())
