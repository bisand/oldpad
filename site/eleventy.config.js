import { readFileSync } from 'node:fs';
import { HtmlBasePlugin } from '@11ty/eleventy';

// The developer pages are the repo's own docs, so there is one copy to keep true.
const repoDocs = { design: 'Design decisions', integrations: 'Writing an integration', 'homey-protocol': 'Homey protocol', 'homey-zone-lights': 'Homey zone lights' };

export default function (config) {
  config.addPlugin(HtmlBasePlugin);
  config.addPassthroughCopy({ 'src/img': 'img', 'src/site.css': 'site.css' });
  config.addPassthroughCopy({ 'node_modules/@fontsource-variable/schibsted-grotesk/files/schibsted-grotesk-latin-wght-normal.woff2': 'font.woff2' });

  for (const [slug, title] of Object.entries(repoDocs)) {
    const body = readFileSync(new URL(`../docs/${slug}.md`, import.meta.url), 'utf8')
      .replace(/^# .*\n/, '')
      .replace(/\]\(([a-z-]+)\.md(#[^)]*)?\)/g, (_, name, hash = '') => `](/dev/${name}/${hash})`);
    config.addTemplate(`dev/${slug}.md`, body, { title, layout: 'doc.njk', permalink: `/dev/${slug}/`, templateEngineOverride: 'md' });
  }

  return { dir: { input: 'src', output: '_site' }, markdownTemplateEngine: 'njk' };
}
