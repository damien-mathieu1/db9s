import { themes as prismThemes } from 'prism-react-renderer';
import type { Config } from '@docusaurus/types';
import type * as Preset from '@docusaurus/preset-classic';

const repo = 'https://github.com/damien-mathieu1/db9s';

const config: Config = {
  title: 'db9s',
  tagline: 'Browse, query and edit your databases and queues, k9s-style',
  favicon: 'img/favicon.svg',

  url: 'https://damien-mathieu1.github.io',
  baseUrl: '/db9s/',
  organizationName: 'damien-mathieu1',
  projectName: 'db9s',
  trailingSlash: false,

  onBrokenLinks: 'throw',

  i18n: {
    defaultLocale: 'en',
    locales: ['en'],
  },

  presets: [
    [
      'classic',
      {
        docs: {
          sidebarPath: './sidebars.ts',
          editUrl: `${repo}/tree/main/website/`,
        },
        blog: false,
        theme: {
          customCss: './src/css/custom.css',
        },
      } satisfies Preset.Options,
    ],
  ],

  themeConfig: {
    colorMode: {
      defaultMode: 'dark',
      respectPrefersColorScheme: true,
    },
    prism: {
      theme: prismThemes.github,
      darkTheme: prismThemes.dracula,
      additionalLanguages: ['bash', 'toml', 'sql', 'json'],
    },
    navbar: {
      title: 'db9s',
      logo: {
        alt: 'db9s logo',
        src: 'img/favicon.svg',
      },
      items: [
        {
          type: 'docSidebar',
          sidebarId: 'docs',
          position: 'left',
          label: 'Documentation',
        },
        {
          href: 'https://crates.io/crates/db9s',
          label: 'crates.io',
          position: 'left',
        },
        {
          type: 'custom-github-stars',
          position: 'right',
        },
      ],
    },
    footer: {
      style: 'dark',
      links: [
        {
          title: 'Docs',
          items: [
            { label: 'Getting started', to: '/docs/intro' },
            { label: 'Installation', to: '/docs/installation' },
            { label: 'Keyboard shortcuts', to: '/docs/usage/keyboard-shortcuts' },
            { label: 'Desktop app', to: '/docs/desktop-app' },
          ],
        },
        {
          title: 'Project',
          items: [
            { label: 'GitHub', href: repo },
            { label: 'Issues', href: `${repo}/issues` },
            { label: 'Releases', href: `${repo}/releases` },
            { label: 'Contributing', href: `${repo}/blob/main/CONTRIBUTING.md` },
          ],
        },
      ],
      copyright: 'MIT licensed. Built with Rust, ratatui and GPUI.',
    },
  } satisfies Preset.ThemeConfig,
};

export default config;
