import { utimesSync } from "node:fs";
import { join } from "node:path";
import { defineConfig, type DefaultTheme } from "vitepress";
import { generate, REPO_URL, ROOT } from "./packs";

// Pack pages are generated from packs/ before every build and dev start.
const generated = generate();

function packSidebar(): DefaultTheme.SidebarItem[] {
  const by = (level: string) => generated.packs.filter((p) => p.level === level).map((p) => ({ text: p.id, link: `/packs/${p.id}` }));
  return [
    { text: "Packs", items: [{ text: "Overview", link: "/packs/" }, { text: "All rules", link: "/reference/rules" }] },
    { text: "User level", items: by("user") },
    { text: "Project level", items: by("project") },
  ];
}

const guide: DefaultTheme.SidebarItem[] = [
  {
    text: "Guide",
    items: [
      { text: "Introduction", link: "/guide/introduction" },
      { text: "Getting started", link: "/guide/getting-started" },
      { text: "How it works", link: "/guide/how-it-works" },
      { text: "Configuration", link: "/guide/configuration" },
      { text: "Checks and hooks", link: "/guide/checks" },
      { text: "Tool audits", link: "/guide/audits" },
      { text: "Writing rules and packs", link: "/guide/authoring" },
    ],
  },
  {
    text: "Reference",
    items: [
      { text: "CLI", link: "/reference/cli" },
      { text: "All rules", link: "/reference/rules" },
      { text: "Specification", link: "/reference/spec" },
    ],
  },
];

export default defineConfig({
  title: "Forge",
  description: "A standalone Claude Code setup, generated from rule packs.",
  base: process.env.DOCS_BASE ?? "/claude-forge/",
  cleanUrls: true,
  lastUpdated: true,
  head: [["link", { rel: "icon", type: "image/svg+xml", href: `${process.env.DOCS_BASE ?? "/claude-forge/"}logo.svg` }]],
  themeConfig: {
    logo: "/logo.svg",
    nav: [
      { text: "Guide", link: "/guide/introduction", activeMatch: "/guide/" },
      { text: "Packs", link: "/packs/", activeMatch: "/packs/" },
      { text: "CLI", link: "/reference/cli" },
      { text: "Spec", link: "/reference/spec" },
    ],
    sidebar: { "/guide/": guide, "/reference/": guide, "/packs/": packSidebar() },
    socialLinks: [{ icon: "github", link: REPO_URL }],
    search: { provider: "local" },
    outline: [2, 3],
    editLink: { pattern: `${REPO_URL}/edit/main/docs/:path`, text: "Edit this page on GitHub" },
    footer: { message: "Pack pages are generated from packs/ on every build." },
  },
  vite: {
    plugins: [
      {
        name: "forge-packs",
        // Dev: regenerate the pages when a pack changes; restart when the set of packs changes (sidebar).
        configureServer(server) {
          const packsDir = join(ROOT, "packs");
          const known = generated.packs.map((p) => p.id).join(",");
          server.watcher.add([packsDir, join(ROOT, "SPECS.md")]);
          let timer: ReturnType<typeof setTimeout> | undefined;
          server.watcher.on("all", (_event, file) => {
            if (!file.startsWith(packsDir) && !file.endsWith("SPECS.md")) return;
            clearTimeout(timer);
            timer = setTimeout(() => {
              try {
                const now = generate().packs.map((p) => p.id).join(",");
                // a new or removed pack changes the sidebar: touching this file makes VitePress reload its config
                if (now !== known) utimesSync(join(ROOT, "docs", ".vitepress", "config.ts"), new Date(), new Date());
              } catch (e) {
                server.config.logger.error(`forge: pack pages not regenerated: ${(e as Error).message}`);
              }
            }, 100);
          });
        },
      },
    ],
  },
});
