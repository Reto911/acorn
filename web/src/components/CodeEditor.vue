<script setup lang="ts">
import { onBeforeUnmount, onMounted, useTemplateRef, watch } from 'vue'
import * as monaco from 'monaco-editor/editor/editor.api'
import EditorWorker from 'monaco-editor/editor/editor.worker?worker'
import 'monaco-editor/languages/definitions/cpp/register'
import 'monaco-editor/languages/definitions/python/register'
import type { LanguageId } from '../api/types'

const props = withDefaults(
  defineProps<{ language: LanguageId; readOnly?: boolean }>(),
  { readOnly: false },
)
const source = defineModel<string>({ required: true })
const emit = defineEmits<{ ready: [] }>()

// Editor code and language definitions load when an editing or source-viewing area mounts.
globalThis.MonacoEnvironment = {
  getWorker: () => new EditorWorker(),
}

const container = useTemplateRef<HTMLDivElement>('container')
let editor: monaco.editor.IStandaloneCodeEditor | undefined
let contentListener: monaco.IDisposable | undefined
let highlights: monaco.editor.IEditorDecorationsCollection | undefined
const models = new Map<LanguageId, monaco.editor.ITextModel>()

/** Remove the current line decoration without changing model contents. */
function clearHighlight() {
  highlights?.clear()
}

/** Reveal a backend-validated inclusive line range for each explicit user action. */
function revealLines(lineStart: number, lineEnd: number) {
  const model = editor?.getModel()
  if (!editor || !model) return
  highlights?.set([
    {
      range: new monaco.Range(
        lineStart,
        1,
        lineEnd,
        model.getLineMaxColumn(lineEnd),
      ),
      options: {
        isWholeLine: true,
        className: 'source-line-highlight',
        linesDecorationsClassName: 'source-line-marker',
      },
    },
  ])
  editor.revealLinesInCenter(lineStart, lineEnd)
  editor.focus()
}

function syncModel() {
  let model = models.get(props.language)
  if (!model) {
    model = monaco.editor.createModel(
      source.value,
      props.language === 'c' ? 'cpp' : 'python',
    )
    models.set(props.language, model)
  } else if (model.getValue() !== source.value) {
    clearHighlight()
    model.setValue(source.value)
  }
  if (editor?.getModel() !== model) {
    clearHighlight()
    editor?.setModel(model)
  }
}

watch(() => [props.language, source.value], syncModel, { flush: 'post' })
defineExpose({ revealLines, clearHighlight })

onMounted(() => {
  editor = monaco.editor.create(container.value!, {
    model: null,
    ariaLabel: props.readOnly ? '本次提交源码（只读）' : '源码编辑器',
    readOnly: props.readOnly,
    domReadOnly: props.readOnly,
    wordWrap: 'off',
    formatOnPaste: false,
    formatOnType: false,
    detectIndentation: false,
    theme: 'vs',
    automaticLayout: true,
    minimap: { enabled: false },
    fontSize: 14,
    scrollBeyondLastLine: false,
    padding: { top: 16 },
  })
  syncModel()
  highlights = editor.createDecorationsCollection()
  if (!props.readOnly)
    contentListener = editor.onDidChangeModelContent(() => {
      source.value = editor!.getValue()
    })
  emit('ready')
})

onBeforeUnmount(() => {
  contentListener?.dispose()
  clearHighlight()
  editor?.dispose()
  editor = undefined
  highlights = undefined
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
.code-editor :deep(.source-line-highlight) {
  background-color: #fef3c7;
}
.code-editor :deep(.source-line-marker) {
  border-left: 3px solid #b45309;
}
</style>
