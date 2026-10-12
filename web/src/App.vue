<script setup lang="ts">
import { ElConfigProvider } from 'element-plus'
import zhCn from 'element-plus/es/locale/lang/zh-cn'
import { RouterLink, RouterView, useRoute } from 'vue-router'

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
        </nav>
      </div>
    </header>
    <main id="main-content" class="main-content" tabindex="-1">
      <RouterView />
    </main>
    <footer class="app-footer">Acorn · C / Python 3 单文件判题</footer>
  </ElConfigProvider>
</template>
