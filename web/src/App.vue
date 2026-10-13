<script setup lang="ts">
import { ElAlert, ElConfigProvider } from 'element-plus'
import zhCn from 'element-plus/es/locale/lang/zh-cn'
import { RouterLink, RouterView, useRoute } from 'vue-router'
import { dismissRuntimeError, runtimeErrorVisible } from './utils/browserErrors'
import 'element-plus/es/components/alert/style/css'

const route = useRoute()
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
      <RouterView />
    </main>
    <footer class="app-footer">Acorn · C / Python 3 单文件判题</footer>
  </ElConfigProvider>
</template>
