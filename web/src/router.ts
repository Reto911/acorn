import { createRouter, createWebHistory } from 'vue-router'

export const router = createRouter({
  history: createWebHistory(import.meta.env.BASE_URL),
  routes: [
    { path: '/', redirect: '/problems' },
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
      meta: { title: '提交历史' },
    },
    {
      path: '/submissions/:submissionId',
      name: 'result',
      component: () => import('./pages/ResultPage.vue'),
      meta: { title: '提交结果' },
    },
    {
      path: '/:pathMatch(.*)*',
      component: () => import('./pages/NotFoundPage.vue'),
      meta: { title: '页面不存在' },
    },
  ],
  scrollBehavior: (_to, _from, savedPosition) => savedPosition ?? { top: 0 },
})

router.afterEach((to) => {
  document.title = `${to.meta.title ?? '代码判题'} · Acorn`
})
