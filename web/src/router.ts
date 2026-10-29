import { createRouter, createWebHistory } from 'vue-router'
import { useAuth } from './composables/useAuth'

export const router = createRouter({
  history: createWebHistory(import.meta.env.BASE_URL),
  routes: [
    { path: '/', redirect: '/problems' },
    {
      path: '/health',
      component: () => import('./pages/HealthPage.vue'),
      meta: { title: '系统状态', requiresAuth: true },
    },
    {
      path: '/problems',
      component: () => import('./pages/ProblemsPage.vue'),
      meta: { title: '题目' },
    },
    {
      path: '/problems/:problemId',
      component: () => import('./pages/SolvePage.vue'),
      meta: { title: '答题' },
    },
    {
      path: '/submissions',
      component: () => import('./pages/SubmissionsPage.vue'),
      meta: { title: '提交历史', requiresAuth: true },
    },
    {
      path: '/submissions/:submissionId',
      name: 'result',
      component: () => import('./pages/ResultPage.vue'),
      meta: { title: '提交结果', requiresAuth: true },
    },
    {
      path: '/:pathMatch(.*)*',
      component: () => import('./pages/NotFoundPage.vue'),
      meta: { title: '页面不存在' },
    },
  ],
  scrollBehavior: (_to, _from, savedPosition) => savedPosition ?? { top: 0 },
})

router.beforeEach(async (to, from) => {
  if (!to.meta.requiresAuth) return true
  const auth = useAuth()
  await auth.initialize()
  if (auth.authenticated.value) return true
  if (auth.status.value === 'ready') auth.openLogin()
  // Initial private links render the login placeholder; cancelled navigation stays put.
  return from.matched.length === 0
})

router.afterEach((to) => {
  document.title = `${to.meta.title ?? '代码判题'} · Acorn`
})
