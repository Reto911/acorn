<script setup lang="ts">
import { onBeforeUnmount, onMounted, useTemplateRef, watch } from 'vue'
import * as monaco from 'monaco-editor/editor/editor.api'
import EditorWorker from 'monaco-editor/editor/editor.worker?worker'
import 'monaco-editor/languages/definitions/cpp/register'
import 'monaco-editor/languages/definitions/python/register'
import type { LanguageId } from '../api/types'

const props = defineProps<{ language: LanguageId }>()
const source = defineModel<string>({ required: true })

// This component and both language definitions load only on the solve route.
globalThis.MonacoEnvironment = {
  getWorker: () => new EditorWorker(),
}

const container = useTemplateRef<HTMLDivElement>('container')
let editor: monaco.editor.IStandaloneCodeEditor | undefined
let contentListener: monaco.IDisposable | undefined
const models = new Map<LanguageId, monaco.editor.ITextModel>()

function syncModel() {
  let model = models.get(props.language)
  if (!model) {
    model = monaco.editor.createModel(
      source.value,
      props.language === 'c' ? 'cpp' : 'python',
    )
    models.set(props.language, model)
  } else if (model.getValue() !== source.value) {
    model.setValue(source.value)
  }
  if (editor?.getModel() !== model) editor?.setModel(model)
}

watch(() => [props.language, source.value], syncModel, { flush: 'post' })

onMounted(() => {
  editor = monaco.editor.create(container.value!, {
    model: null,
    ariaLabel: '源码编辑器',
    detectIndentation: false,
    theme: 'vs',
    automaticLayout: true,
    minimap: { enabled: false },
    fontSize: 14,
    scrollBeyondLastLine: false,
    padding: { top: 16 },
  })
  syncModel()
  contentListener = editor.onDidChangeModelContent(() => {
    source.value = editor!.getValue()
  })
})

onBeforeUnmount(() => {
  contentListener?.dispose()
  editor?.dispose()
  for (const model of models.values()) model.dispose()
  models.clear()
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
