<script setup lang="ts">
import { ElAlert, ElButton, ElConfigProvider } from 'element-plus'
import { watch } from 'vue'
import zhCn from 'element-plus/es/locale/lang/zh-cn'
import { RouterLink, RouterView, useRoute } from 'vue-router'
import { dismissRuntimeError, runtimeErrorVisible } from './utils/browserErrors'
import { useAuth } from './composables/useAuth'
import { errorFeedback } from './api/feedback'
import AuthDialog from './components/AuthDialog.vue'
import 'element-plus/es/components/alert/style/css'
import 'element-plus/es/components/button/style/css'

const route = useRoute()
const auth = useAuth()
const { user, status, generation, loadError, busy, formError } = auth
void auth.initialize()
watch(
  () => [route.meta.requiresAuth, status.value, user.value?.id],
  () => {
    if (route.meta.requiresAuth && status.value === 'ready' && !user.value)
      auth.openLogin()
  },
  { immediate: true },
)
</script>

<template>
  <ElConfigProvider :locale="zhCn">
    <a class="skip-link" href="#main-content">跳到主要内容</a>
    <header class="app-header">
      <div class="header-inner">
        <RouterLink class="brand" to="/problems" aria-label="Acorn 题目首页">
          <span class="brand-mark" aria-hidden="true">a</span>
          <span>Acorn</span>
          <span class="brand-caption">代码判题</span>
        </RouterLink>
        <nav class="main-nav" aria-label="主导航">
          <RouterLink
            to="/problems"
            :class="{ 'is-active': route.path.startsWith('/problems') }"
            :aria-current="
              route.path.startsWith('/problems') ? 'page' : undefined
            "
          >
            题目
          </RouterLink>
          <RouterLink
            to="/submissions"
            :class="{ 'is-active': route.path.startsWith('/submissions') }"
            :aria-current="
              route.path.startsWith('/submissions') ? 'page' : undefined
            "
          >
            提交历史
          </RouterLink>
          <RouterLink
            to="/health"
            :class="{ 'is-active': route.path === '/health' }"
            :aria-current="route.path === '/health' ? 'page' : undefined"
          >
            系统状态
          </RouterLink>
        </nav>
        <div class="account-nav">
          <template v-if="user">
            <span
              >{{ user.display_name }} <small>@{{ user.username }}</small></span
            >
            <ElButton :loading="busy" @click="auth.logout">退出</ElButton>
          </template>
          <span v-else-if="status === 'loading' || status === 'unknown'"
            >正在确认身份…</span
          >
          <ElButton v-else @click="auth.openLogin">登录 / 注册</ElButton>
        </div>
      </div>
    </header>
    <main id="main-content" class="main-content" tabindex="-1">
      <ElAlert
        v-if="runtimeErrorVisible"
        class="runtime-error"
        title="页面遇到错误，部分操作可能未完成"
        description="请先保留当前编辑内容，再重试刚才的操作；若持续出现，请联系维护者。"
        type="error"
        show-icon
        @close="dismissRuntimeError"
      />
      <ElAlert
        v-if="loadError"
        title="未能确认当前登录状态"
        type="error"
        :closable="false"
        show-icon
        class="runtime-error"
      >
        <ElButton :loading="status === 'loading'" @click="auth.refresh"
          >重新确认身份</ElButton
        >
      </ElAlert>
      <ElAlert
        v-if="formError && !auth.dialogOpen.value"
        :title="errorFeedback(formError).title"
        :description="errorFeedback(formError).description"
        type="error"
        show-icon
        class="runtime-error"
        @close="auth.clearFormError"
      />
      <RouterView v-slot="{ Component }">
        <component
          :is="Component"
          v-if="!route.meta.requiresAuth || (status === 'ready' && user)"
          :key="route.meta.requiresAuth ? generation : undefined"
        />
        <section v-else class="surface auth-placeholder">
          <h1>
            {{
              status === 'loading' || status === 'unknown'
                ? '正在确认身份…'
                : '登录后查看'
            }}
          </h1>
          <p>登录后可查看个人提交、结果和系统状态。</p>
          <ElButton
            v-if="status === 'ready'"
            type="primary"
            @click="auth.openLogin"
            >登录 / 注册</ElButton
          >
          <ElButton v-else-if="status === 'error'" @click="auth.refresh"
            >重新确认身份</ElButton
          >
        </section>
      </RouterView>
    </main>
    <footer class="app-footer">Acorn · 2026</footer>
    <AuthDialog />
  </ElConfigProvider>
</template>
