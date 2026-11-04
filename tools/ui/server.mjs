// Deterministic fixture server for the Clipping Factory studio.
//
// Serves the real static/ bundle and answers every /api route from
// fixtures/state.mjs. The state comes from the X-UI-State request header
// (Playwright sets it on the whole context) or a cf-ui-state cookie when a
// person is browsing by hand: `npm run serve` then open /?state=results-6-clips.

import { createServer } from "node:http";
import { readFile, stat } from "node:fs/promises";
import path from "node:path";
import { fileURLToPath } from "node:url";
import { fixtureFor, clipFor, frameIndexFor, PROJECT_ID } from "./fixtures/state.mjs";

const ROOT = path.dirname(fileURLToPath(import.meta.url));
const STATIC_DIR = path.resolve(ROOT, "..", "..", "static");
const FRAME_DIR = path.join(ROOT, "fixtures", "frames");

const MIME = {
  ".html": "text/html; charset=utf-8",
  ".css": "text/css; charset=utf-8",
  ".js": "text/javascript; charset=utf-8",
  ".json": "application/json; charset=utf-8",
  ".png": "image/png",
  ".svg": "image/svg+xml",
  ".ico": "image/x-icon",
};

function send(res, status, type, body, extra = {}) {
  res.writeHead(status, { "Content-Type": type, "Cache-Control": "no-store", ...extra });
  res.end(body);
}

function json(res, status, value) {
  send(res, status, "application/json; charset=utf-8", JSON.stringify(value));
}

function stateName(req, url) {
  const header = req.headers["x-ui-state"];
  if (header) return String(header);
  const cookie = req.headers.cookie || "";
  const match = cookie.match(/(?:^|;\s*)cf-ui-state=([^;]+)/);
  if (match) return decodeURIComponent(match[1]);
  return url.searchParams.get("state") || "empty-upload";
}

async function staticFile(res, name) {
  const file = path.join(STATIC_DIR, name);
  try {
    const body = await readFile(file);
    send(res, 200, MIME[path.extname(file)] || "application/octet-stream", body);
  } catch {
    send(res, 404, "text/plain; charset=utf-8", "not found");
  }
}

async function frame(res, index) {
  try {
    const body = await readFile(path.join(FRAME_DIR, `frame-${index}.png`));
    send(res, 200, "image/png", body);
  } catch {
    send(res, 404, "text/plain; charset=utf-8", "missing frame");
  }
}

function eventStream(req, res, fixture) {
  res.writeHead(200, {
    "Content-Type": "text/event-stream",
    "Cache-Control": "no-cache, no-transform",
    Connection: "keep-alive",
  });
  if (fixture.view) {
    res.write(`data: ${JSON.stringify({ type: "snapshot", view: fixture.view })}\n\n`);
    if (fixture.live) res.write(`data: ${JSON.stringify({ type: "progress", ...fixture.live })}\n\n`);
  }
  const ping = setInterval(() => res.write(": ping\n\n"), 15_000);
  req.on("close", () => clearInterval(ping));
  req.on("error", () => clearInterval(ping));
}

export async function createFixtureServer({ host = "127.0.0.1", port = 0 } = {}) {
  const server = createServer(async (req, res) => {
    try {
      const url = new URL(req.url, `http://${req.headers.host || "127.0.0.1"}`);
      const parts = url.pathname.split("/").filter(Boolean);
      const fixture = fixtureFor(stateName(req, url));

      if (url.pathname === "/__health") return json(res, 200, { ok: true });

      if (url.pathname === "/" || url.pathname === "/index.html") {
        if (url.searchParams.get("state")) {
          res.setHeader("Set-Cookie", `cf-ui-state=${encodeURIComponent(url.searchParams.get("state"))}; Path=/`);
        }
        return staticFile(res, "index.html");
      }
      if (url.pathname === "/styles.css") return staticFile(res, "styles.css");
      if (url.pathname === "/app.js") return staticFile(res, "app.js");
      if (url.pathname === "/review.js") return staticFile(res, "review.js");
      if (parts[0] === "fixtures" && parts[1] === "frames" && parts[2]) {
        const file = path.basename(parts[2]);
        try {
          const body = await readFile(path.join(FRAME_DIR, file));
          return send(res, 200, "image/png", body);
        } catch {
          return send(res, 404, "text/plain; charset=utf-8", "missing frame");
        }
      }

      if (url.pathname === "/api/setup" && req.method === "GET") return json(res, 200, fixture.setup);
      if (url.pathname === "/api/settings/ai" && req.method === "GET") return json(res, 200, fixture.settings);
      if (url.pathname === "/api/settings/ai" && req.method === "POST") return json(res, 200, fixture.settings);
      if (url.pathname === "/api/projects" && req.method === "GET") return json(res, 200, fixture.library);

      if (parts[0] === "api" && parts[1] === "projects" && parts[2]) {
        const id = decodeURIComponent(parts[2]);
        const rest = parts.slice(3);
        if (rest.length === 0 && req.method === "GET") {
          if (!fixture.view || id !== PROJECT_ID) return json(res, 404, { error: "Project not found." });
          return json(res, 200, fixture.view);
        }
        if (rest.length === 1 && rest[0] === "events") {
          if (!fixture.view || id !== PROJECT_ID) return json(res, 404, { error: "Project not found." });
          return eventStream(req, res, fixture);
        }
        if (rest.length === 1 && ["cancel", "retry", "process", "open-output-folder"].includes(rest[0]) && req.method === "POST") {
          return json(res, 200, { started: true, cancelled: false, opened: true });
        }
        if (rest[0] === "clips" && rest[1]) {
          const clip = clipFor(decodeURIComponent(rest[1]));
          if (!clip) return json(res, 404, { error: "Clip not found." });
          const tail = rest.slice(2);
          if (tail.length === 0) return json(res, 404, { error: "Clip media is not part of this fixture." });
          if (tail[0] === "export" && tail[1] === "poster") return frame(res, frameIndexFor(clip.id));
          if (tail[0] === "export") {
            return send(res, 200, "text/plain; charset=utf-8", `${clip.headline}\n00:00-00:37\nFixture export for UI review.\n`);
          }
          if (tail[0] === "download") {
            return send(res, 200, "text/plain; charset=utf-8", "Fixture download placeholder.\n");
          }
          if (tail[0] === "restyle" && req.method === "POST") return json(res, 200, clip);
        }
      }

      return json(res, 404, { error: `No fixture route for ${req.method} ${url.pathname}` });
    } catch (error) {
      return json(res, 500, { error: String(error && error.stack ? error.stack : error) });
    }
  });

  await new Promise((resolve, reject) => {
    server.once("error", reject);
    server.listen(port, host, resolve);
  });
  const address = server.address();
  const base = `http://${host}:${address.port}`;
  return {
    url: base,
    port: address.port,
    async close() {
      server.closeAllConnections?.();
      await new Promise((resolve) => server.close(resolve));
    },
  };
}

// `node server.mjs --port 4573` for manual browsing.
if (process.argv[1] && fileURLToPath(import.meta.url) === path.resolve(process.argv[1])) {
  const portFlag = process.argv.indexOf("--port");
  const port = portFlag > -1 ? Number(process.argv[portFlag + 1]) : 4573;
  const instance = await createFixtureServer({ port });
  console.log(`fixture studio: ${instance.url}/?state=results-6-clips`);
}
