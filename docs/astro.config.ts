import { utimesSync } from "node:fs";
import { join } from "node:path";
import react from "@astrojs/react";
import starlight from "@astrojs/starlight";
import { defineConfig } from "astro/config";
import type { AstroIntegration } from "astro";
import { leaf, packs, REPO_URL, ROOT, spec } from "./src/lib/data";
import type { Pack } from "../src/types";

type SidebarItem = { label: string; link: string } | { label: string; collapsed?: boolean; items: SidebarItem[] };

/** The pack tree as nested sidebar groups: a pack with children is a group whose first link is the pack itself. */
function packItems(all: Map<string, Pack>, ids: string[]): SidebarItem[] {
  return ids.map((id) => {
    const p = all.get(id)!;
    const link = { label: leaf(id), link: `/packs/${id}/` };
    if (!p.children.length) return link;
    return { label: leaf(id), collapsed: p.parent !== undefined, items: [{ label: `${leaf(id)} (overview)`, link: link.link }, ...packItems(all, p.children)] };
  });
}

function sidebar(): SidebarItem[] {
  const all = packs();
  const roots = [...all.values()].filter((p) => !p.parent);
  return [
    {
      label: "Guide",
      items: [
        { label: "Introduction", link: "/guide/introduction/" },
        { label: "Getting started", link: "/guide/getting-started/" },
        { label: "Configuration", link: "/guide/configuration/" },
        { label: "Writing packs", link: "/guide/writing-packs/" },
      ],
    },
    {
      label: "Specification",
      items: [{ label: "Overview and results", link: "/spec/" }, ...spec().map((s) => ({ label: `${s.number} · ${s.title}`, link: `/spec/${s.slug}/` }))],
    },
    {
      label: "Packs",
      items: [
        { label: "Pack tree", link: "/packs/" },
        { label: "All rules", link: "/packs/rules/" },
        ...packItems(all, roots.filter((p) => p.level === "project").map((p) => p.id)),
        { label: "User level", items: packItems(all, roots.filter((p) => p.level === "user").map((p) => p.id)) },
      ],
    },
    {
      label: "Reference",
      items: [
        { label: "CLI", link: "/reference/cli/" },
        { label: "Schemas", link: "/reference/schemas/" },
      ],
    },
  ];
}

/** Dev: pages re-read packs/ and tests/ on each request; adding or removing a pack or spec file restarts the server so the sidebar follows. */
const forgeWatch: AstroIntegration = {
  name: "forge-watch",
  hooks: {
    "astro:server:setup": ({ server }) => {
      server.watcher.add([join(ROOT, "packs"), join(ROOT, "tests", "spec")]);
      const structural = (file: string) => file.endsWith("pack.yaml") || (file.includes("/tests/spec/") && file.endsWith(".test.ts"));
      const restart = (file: string) => {
        if (structural(file)) utimesSync(join(ROOT, "docs", "astro.config.ts"), new Date(), new Date());
      };
      server.watcher.on("add", restart);
      server.watcher.on("unlink", restart);
    },
  },
};

export default defineConfig({
  site: process.env.DOCS_SITE ?? "https://mde-pach.github.io",
  base: process.env.DOCS_BASE ?? "/claude-forge",
  trailingSlash: "always",
  integrations: [
    starlight({
      title: "Forge",
      description: "Guardrails for Claude Code, generated from rule packs.",
      logo: { src: "./public/logo.svg" },
      favicon: "/logo.svg",
      social: [{ icon: "github", label: "GitHub", href: REPO_URL }],
      editLink: { baseUrl: `${REPO_URL}/edit/main/docs/` },
      customCss: ["./src/styles/custom.css"],
      sidebar: sidebar(),
      lastUpdated: true,
    }),
    react(),
    forgeWatch,
  ],
});
