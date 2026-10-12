<script setup lang="ts">
import { onBeforeUnmount, onMounted, useTemplateRef } from 'vue'
import * as monaco from 'monaco-editor/editor/editor.api'
import EditorWorker from 'monaco-editor/editor/editor.worker?worker'

// This component is loaded only on the solve route. F04 adds editable models.
globalThis.MonacoEnvironment = {
  getWorker: () => new EditorWorker(),
}

const container = useTemplateRef<HTMLDivElement>('container')
let editor: monaco.editor.IStandaloneCodeEditor | undefined

onMounted(() => {
  editor = monaco.editor.create(container.value!, {
    value: '',
    language: 'plaintext',
    readOnly: true,
    domReadOnly: true,
    ariaLabel: '代码编辑区，暂未开放编辑',
    theme: 'vs',
    automaticLayout: true,
    minimap: { enabled: false },
    fontSize: 14,
    scrollBeyondLastLine: false,
    padding: { top: 16 },
  })
})

onBeforeUnmount(() => {
  const model = editor?.getModel()
  editor?.dispose()
  model?.dispose()
})
</script>

<template>
  <div ref="container" class="code-editor" />
</template>

<style scoped>
.code-editor {
  min-width: 0;
  height: 480px;
}
</style>
