const INSTALLER =
  "https://github.com/ivov/lisette/releases/latest/download/lisette-installer.sh";

const REDIRECTS = {
  "/docs": "/docs/intro/quickstart/",
  "/docs/": "/docs/intro/quickstart/",
  "/quickstart": "/docs/intro/quickstart/",
  "/quickstart/": "/docs/intro/quickstart/",
};

export default {
  async fetch(request, env) {
    const { pathname } = new URL(request.url);

    const target = REDIRECTS[pathname];
    if (target) return Response.redirect(new URL(target, request.url), 302);

    if (pathname === "/install.sh") {
      const upstream = await fetch(INSTALLER, { redirect: "follow" });
      if (!upstream.ok) {
        return new Response(`Installer unavailable (${upstream.status})\n`, {
          status: 502,
          headers: { "content-type": "text/plain; charset=utf-8" },
        });
      }
      return new Response(upstream.body, {
        headers: {
          "content-type": "text/plain; charset=utf-8",
          "cache-control": "public, max-age=300",
        },
      });
    }

    return env.ASSETS.fetch(request);
  },
};
