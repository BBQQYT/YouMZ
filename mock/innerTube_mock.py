import json, os
from http.server import BaseHTTPRequestHandler, HTTPServer

MOCK_TRACKS = [
    ("mock1", "Тестовый трек 440", "Мок Артист", "0:03", "tone1.m4a"),
    ("mock_ad", "Рекламный трек", "Реклама ООО", "0:05", None),   # player вернёт UNPLAYABLE
    ("mock2", "Тестовый трек 330", "Мок Артист", "0:03", "tone2.m4a"),
    ("mock3", "Тестовый трек 550", "Мок Артист", "0:03", "tone3.m4a"),
]

def panel_item(vid, title, artist, length):
    return {"playlistPanelVideoRenderer": {
        "videoId": vid,
        "title": {"simpleText": title},
        "longBylineText": {"runs": [{"text": artist}]},
        "thumbnail": {"thumbnails": [{"url": f"http://127.0.0.1:8099/art.jpg"}]},
        "lengthText": {"simpleText": length},
    }}

class H(BaseHTTPRequestHandler):
    def _send(self, obj, code=200):
        data = json.dumps(obj).encode()
        self.send_response(code)
        self.send_header("Content-Type", "application/json")
        self.send_header("Content-Length", str(len(data)))
        self.end_headers()
        self.wfile.write(data)

    def do_POST(self):
        n = int(self.headers.get("Content-Length", 0))
        body = json.loads(self.rfile.read(n) or b"{}")
        path = self.path.split("?")[0]
        if path.endswith("/next"):
            items = [
                panel_item(vid, t, a, l) for vid, t, a, l, _ in MOCK_TRACKS
            ]
            # рекламные элементы, которые приложение должно вырезать
            items.insert(1, {"adSlotRenderer": {"slotId": "slot1"}})
            items.insert(3, {"playlistPanelVideoWrapperRenderer": {"adRenderer": {}}})
            items.append({"playlistPanelVideoRenderer": {"videoId": "", "title": {"simpleText": "пустой"}}})
            self._send({"contents": {"twoColumnWatchNextResults": {
                "playlist": {"playlistPanelVideoRenderer": items}}}})
        elif path.endswith("/player"):
            vid = body.get("videoId", "")
            entry = next((m for m in MOCK_TRACKS if m[0] == vid), None)
            if entry and entry[4]:
                self._send({
                    "playabilityStatus": {"status": "OK"},
                    "videoDetails": {"lengthSeconds": "3", "title": entry[1]},
                    "streamingData": {"adaptiveFormats": [
                        {"itag": 140, "mimeType": "audio/mp4; codecs=\"mp4a.40.2\"",
                         "url": f"http://127.0.0.1:8099/{entry[4]}"},
                        {"itag": 251, "mimeType": "audio/webm; codecs=\"opus\"",
                         "url": f"http://127.0.0.1:8099/{entry[4].replace('.m4a','.webm')}"},
                    ]},
                })
            else:
                # реклама / недоступный контент
                self._send({"playabilityStatus": {"status": "UNPLAYABLE",
                            "reason": "Видео недоступно (реклама)"}})
            self.wfile.flush()
        else:
            self._send({"error": "unknown"}, 404)

    def do_GET(self):
        fn = os.path.basename(self.path)
        if fn in ("tone1.m4a", "tone2.m4a", "tone3.m4a") and os.path.exists(fn):
            data = open(fn, "rb").read()
            self.send_response(200)
            self.send_header("Content-Type", "audio/mp4")
            self.send_header("Content-Length", str(len(data)))
            self.end_headers()
            self.wfile.write(data)
        else:
            self.send_response(404)
            self.end_headers()

    def log_message(self, *a):
        pass

HTTPServer(("127.0.0.1", 8099), H).serve_forever()
