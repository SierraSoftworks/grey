import { defineUserConfig, PageHeader } from 'vuepress'
import { viteBundler } from "@vuepress/bundler-vite";
import { defaultTheme } from "@vuepress/theme-default";
import { path } from '@vuepress/utils'

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
    ['link', { rel: 'icon', href: '/favicon.ico' }],
    ["script", {
        defer: "",
        src: "https://analytics.sierrasoftworks.com/script.js",
        "data-website-id": "75074736-b79c-4060-aa8e-a3297b0e61ba",
    }],
  ],

  extendsPage(page, _app) {
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
          '/targets/grpc.md',
          '/targets/http.md',
          '/targets/script.md',
          '/targets/tcp.md',
        ]
      },
      {
        text: "Checks",
        link: "/checks/",
      },
      {
        text: "User Interface",
        link: "/ui/",
        children: [
          '/ui/README.md',
          '/ui/links.md',
          '/ui/incidents.md',
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
            '/guide/clustering.md',
            '/guide/crons.md',
            '/guide/webhooks.md',
            '/guide/telemetry.md',
            '/guide/azure-msi.md',
          ]
        }
      ],
      '/targets/': [
        {
          text: "Targets",
          children: [
            '/targets/README.md',
            '/targets/dns.md',
            '/targets/grpc.md',
            '/targets/http.md',
            '/targets/script.md',
            '/targets/tcp.md',
          ]
        }
      ],
      '/checks/': [
        {
          text: "Checks",
          children: [
            '/checks/README.md',
          ]
        }
      ],
      '/ui/': [
        {
          text: "User Interface",
          children: [
            '/ui/README.md',
            '/ui/links.md',
            '/ui/incidents.md',
          ]
        }
      ]
    }
  }),

  plugins: [
    registerComponentsPlugin({
      componentsDir: path.resolve(__dirname, './components'),
    })
  ]
})
