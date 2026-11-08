// Serves /packs/<name>.neopack and /packs/<name>.zip from R2 (key packs/<name>), GET and HEAD,
// with ranges and conditional requests. The names carry the version (hi-2026.11.06.neopack,
// english-base-<sha256 prefix>.zip), so a file never changes and is cached for a year.
// Every other path is a static asset and never reaches this code (run_worker_first).

const FILE = /^\/packs\/([a-z0-9][a-z0-9._-]*\.(?:neopack|zip))$/;
const IMMUTABLE = "public, max-age=31536000, immutable";

export default {
  async fetch(request, env) {
    const url = new URL(request.url);
    const match = FILE.exec(url.pathname);
    if (!match) return env.ASSETS.fetch(request);
    if (request.method !== "GET" && request.method !== "HEAD") {
      return new Response("Method Not Allowed", { status: 405, headers: { Allow: "GET, HEAD" } });
    }
    const name = match[1];
    const key = `packs/${name}`;

    if (request.method === "HEAD") {
      const object = await env.PACKS.head(key);
      if (object === null) return new Response(null, { status: 404 });
      const headers = fileHeaders(object, name);
      headers.set("Content-Length", String(object.size));
      return new Response(null, { headers });
    }

    let object;
    try {
      object = await env.PACKS.get(key, { onlyIf: request.headers, range: request.headers });
    } catch (error) {
      // An unsatisfiable or malformed Range.
      const head = await env.PACKS.head(key);
      if (head === null) return new Response("Not Found", { status: 404 });
      console.log(JSON.stringify({ message: "range refused", key, range: request.headers.get("Range"), error: String(error) }));
      return new Response("Range Not Satisfiable", { status: 416, headers: { "Content-Range": `bytes */${head.size}` } });
    }
    if (object === null) return new Response("Not Found", { status: 404 });

    const headers = fileHeaders(object, name);
    if (!("body" in object)) {
      // A precondition failed: If-None-Match matched (304), or If-Match and the like didn't (412).
      const notModified = request.headers.has("If-None-Match") || request.headers.has("If-Modified-Since");
      return new Response(null, { status: notModified ? 304 : 412, headers });
    }
    if (request.headers.has("Range") && object.range) {
      const [start, end] = span(object.range, object.size);
      headers.set("Content-Range", `bytes ${start}-${end}/${object.size}`);
      headers.set("Content-Length", String(end - start + 1));
      return new Response(object.body, { status: 206, headers });
    }
    headers.set("Content-Length", String(object.size));
    return new Response(object.body, { headers });
  },
};

function fileHeaders(object, name) {
  const headers = new Headers();
  object.writeHttpMetadata(headers);
  headers.set("Content-Type", "application/zip");
  headers.set("Content-Disposition", `attachment; filename="${name}"`);
  headers.set("Cache-Control", IMMUTABLE);
  headers.set("ETag", object.httpEtag);
  headers.set("Last-Modified", object.uploaded.toUTCString());
  headers.set("Accept-Ranges", "bytes");
  return headers;
}

// The first and last byte an R2Range covers.
function span(range, size) {
  if (range.suffix !== undefined) return [Math.max(0, size - range.suffix), size - 1];
  const start = range.offset ?? 0;
  const end = range.length !== undefined ? Math.min(size - 1, start + range.length - 1) : size - 1;
  return [start, end];
}
