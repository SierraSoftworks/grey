import { defineUserConfig, PageHeader } from 'vuepress'
import { viteBundler } from "@vuepress/bundler-vite";
import { defaultTheme } from "@vuepress/theme-default";
import { path } from '@vuepress/utils'

import { googleAnalyticsPlugin } from '@vuepress/plugin-google-analytics'
import { registerComponentsPlugin } from '@vuepress/plugin-register-components'

function htmlDecode(input: string): string {
  return input.replace("&#39;", "'").replace("&amp;", "&").replace("&quot;", '"')
}

function fixPageHeader(header: PageHeader) {
  header.title = htmlDecode(header.title)
  header.children.forEach(child => fixPageHeader(child))
}

export default defineUserConfig({
  lang: 'en-GB',
  title: 'Grey',
  description: 'Best in class health probing for all of your production endpoints.',

  head: [
    ['meta', { name: "description", content: "Documentation for Grey, a lightweight health probing agent with native OpenTelemetry integration." }],
    ['link', { rel: 'icon', href: '/favicon.ico' }]
  ],

  extendsPage(page, app) {
    const fixedHeaders = page.headers || []
    fixedHeaders.forEach(header => fixPageHeader(header))
  },

  bundler: viteBundler(),

  theme: defaultTheme({
    logo: 'https://cdn.sierrasoftworks.com/logos/icon.png',
    logoDark: 'https://cdn.sierrasoftworks.com/logos/icon_light.png',

    repo: "SierraSoftworks/grey",
    docsRepo: "SierraSoftworks/grey",
    docsDir: 'docs',
    navbar: [
      {
        text: "Getting Started",
        link: "/guide/",
      },
      {
        text: "Targets",
        link: "/targets/",
        children: [
          '/targets/README.md',
          '/targets/dns.md',
          '/targets/http.md',
          '/targets/tcp.md',
        ]
      },
      {
        text: "Validators",
        link: "/validators/",
        children: [
          '/validators/README.md',
          '/validators/contains.md',
          '/validators/equals.md',
          '/validators/one_of.md',
        ]
      },
      {
        text: "Download",
        link: "https://github.com/SierraSoftworks/grey/releases",
        target: "_blank"
      },
      {
        text: "Report an Issue",
        link: "https://github.com/SierraSoftworks/grey/issues/new",
        target: "_blank"
      }
    ],

    sidebar: {
      '/guide/': [
        {
          text: "Getting Started",
          children: [
            '/guide/README.md',
            `/guide/configuration.md`,
            '/guide/telemetry.md',
          ]
        }
      ],
      '/targets/': [
        {
          text: "Targets",
          children: [
            '/targets/README.md',
            '/targets/dns.md',
            '/targets/http.md',
            '/targets/tcp.md',
          ]
        }
      ],
      '/validators/': [
        {
          text: "Validators",
          children: [
            '/validators/README.md',
            '/validators/contains.md',
            '/validators/equals.md',
            '/validators/one_of.md',
          ]
        }
      ]
    }
  }),

  plugins: [
    googleAnalyticsPlugin({ id: "G-WJQ1PVYVH0" }),
    registerComponentsPlugin({
      componentsDir: path.resolve(__dirname, './components'),
    })
  ]
})