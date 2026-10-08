from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from pathlib import Path
import sys
import time

FAST = b"jelly-download-fixture\n"
SLOW_CHUNK = b"x" * 65536
SLOW_CHUNKS = 256

class Handler(BaseHTTPRequestHandler):
    def do_GET(self):
        if self.path == "/":
            body = b"""<!doctype html><meta charset="utf-8"><title>Browser state fixture</title>
<a id="fast" download href="/fast">Fast download</a>
<a id="slow" download href="/slow">Slow download</a>
"""
            self.send_response(200)
            self.send_header("Content-Type", "text/html; charset=utf-8")
            self.send_header("Content-Length", str(len(body)))
            self.send_header("Cache-Control", "no-store")
            self.end_headers()
            self.wfile.write(body)
            return
        if self.path == "/fast":
            self.send_response(200)
            self.send_header("Content-Type", "text/plain")
            self.send_header("Content-Disposition", 'attachment; filename="fixture.txt"')
            self.send_header("Content-Length", str(len(FAST)))
            self.end_headers()
            self.wfile.write(FAST)
            return
        if self.path == "/slow":
            total = len(SLOW_CHUNK) * SLOW_CHUNKS
            self.send_response(200)
            self.send_header("Content-Type", "application/octet-stream")
            self.send_header("Content-Disposition", 'attachment; filename="slow.bin"')
            self.send_header("Content-Length", str(total))
            self.end_headers()
            try:
                for _ in range(SLOW_CHUNKS):
                    self.wfile.write(SLOW_CHUNK)
                    self.wfile.flush()
                    time.sleep(0.10)
            except (BrokenPipeError, ConnectionResetError):
                pass
            return
        self.send_error(404)

    def log_message(self, format, *args):
        pass

server = ThreadingHTTPServer(("127.0.0.1", 0), Handler)
Path(sys.argv[1]).write_text(str(server.server_port))
server.serve_forever()
