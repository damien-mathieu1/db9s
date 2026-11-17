import type { SidebarsConfig } from '@docusaurus/plugin-content-docs';

const sidebars: SidebarsConfig = {
  docs: [
    'intro',
    'installation',
    {
      type: 'category',
      label: 'Usage',
      collapsed: false,
      items: [
        'usage/navigation',
        'usage/keyboard-shortcuts',
        'usage/command-bar',
        'usage/querying',
        'usage/editing',
        'usage/sort-page-export',
        'usage/schema',
      ],
    },
    {
      type: 'category',
      label: 'Databases',
      items: ['databases/sql', 'databases/mongodb', 'databases/redis', 'databases/rabbitmq'],
    },
    'desktop-app',
    {
      type: 'category',
      label: 'Configuration',
      items: [
        'configuration/config-file',
        'configuration/secrets',
        'configuration/read-only',
        'configuration/themes',
      ],
    },
    'faq',
    {
      type: 'link',
      label: 'Contributing',
      href: 'https://github.com/damien-mathieu1/db9s/blob/main/CONTRIBUTING.md',
    },
  ],
};

export default sidebars;
