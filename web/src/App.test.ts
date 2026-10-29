import { createSSRApp, defineComponent, h, ref } from 'vue'
import { renderToString } from 'vue/server-renderer'
import { createMemoryHistory, createRouter } from 'vue-router'
import { beforeEach, describe, expect, it, vi } from 'vitest'
import App from './App.vue'
import type { User } from './api/types'

const auth = {
  user: ref<User | null>(null),
  status: ref('unknown'),
  generation: ref(0),
  loadError: ref(null),
  formError: ref(null),
  busy: ref(false),
  dialogOpen: ref(false),
  initialize: vi.fn(),
  openLogin: vi.fn(),
  refresh: vi.fn(),
  logout: vi.fn(),
  clearFormError: vi.fn(),
}

vi.mock('./composables/useAuth', () => ({ useAuth: () => auth }))
vi.mock('./components/AuthDialog.vue', () => ({
  default: { render: () => null },
}))
vi.mock('element-plus/es/components/alert/style/css', () => ({}))
vi.mock('element-plus/es/components/button/style/css', () => ({}))

beforeEach(() => {
  vi.clearAllMocks()
  auth.user.value = null
  auth.status.value = 'unknown'
})

async function renderPage(protectedPage: boolean) {
  const mount = vi.fn()
  const router = createRouter({
    history: createMemoryHistory(),
    routes: [
      {
        path: '/page',
        meta: { requiresAuth: protectedPage },
        component: defineComponent({
          setup() {
            mount()
            return () => h('p', 'page-content')
          },
        }),
      },
    ],
  })
  await router.push('/page')
  const html = await renderToString(createSSRApp(App).use(router))
  return { mount, html }
}

describe('private page mounting', () => {
  it.each(['unknown', 'loading', 'error', 'ready'])(
    'does not mount private content with %s identity and no confirmed user',
    async (status) => {
      auth.status.value = status
      const { mount, html } = await renderPage(true)
      expect(mount).not.toHaveBeenCalled()
      expect(html).not.toContain('page-content')
      expect(html).toContain(
        status === 'loading' || status === 'unknown'
          ? '正在确认身份'
          : '登录后查看',
      )
    },
  )

  it('mounts public content while identity is unknown and private content after login', async () => {
    const publicPage = await renderPage(false)
    expect(publicPage.mount).toHaveBeenCalledOnce()
    auth.user.value = { id: 1, username: 'test', display_name: 'Test' }
    auth.status.value = 'ready'
    const privatePage = await renderPage(true)
    expect(privatePage.mount).toHaveBeenCalledOnce()
    expect(privatePage.html).toContain('page-content')
    expect(privatePage.html).toContain('@test')
  })
})
