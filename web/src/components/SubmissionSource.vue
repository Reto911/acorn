<script setup lang="ts">
import {
  computed,
  defineAsyncComponent,
  ref,
  shallowRef,
  useTemplateRef,
  watch,
} from 'vue'
import { getSubmissionSource } from '../api'
import { useAuth, type Identity } from '../composables/useAuth'
import { useQuery } from '../composables/useQuery'
import {
  sourcePresentation,
  type SourceLocation,
} from '../utils/sourcePresentation'
import RequestFeedback from './RequestFeedback.vue'

const props = defineProps<{
  submissionId: string
  identity: Identity
  location: SourceLocation | null
}>()
const auth = useAuth()
const {
  data: source,
  error,
  loading,
  reload,
  stop,
} = useQuery(
  () => props.submissionId,
  (id, signal) =>
    getSubmissionSource(id, {
      signal,
      expectedUserId: props.identity.userId,
    }),
  { handleError: (cause) => auth.handleError(cause, props.identity) },
)
const display = computed(() =>
  source.value ? sourcePresentation(source.value.source) : null,
)
const CodeEditor = defineAsyncComponent(() => import('./CodeEditor.vue'))
const editor = useTemplateRef<InstanceType<typeof CodeEditor>>('editor')
const panel = useTemplateRef<HTMLElement>('panel')
const editorReady = ref(false)
const located = shallowRef<SourceLocation | null>(null)

/** Apply only the latest explicit selection after both the source and model are ready. */
function applyLocation() {
  if (!auth.isCurrent(props.identity)) return
  const location = props.location
  if (!location) {
    editor.value?.clearHighlight()
    located.value = null
    return
  }
  if (
    !editorReady.value ||
    !editor.value ||
    !source.value ||
    loading.value ||
    error.value
  )
    return
  panel.value?.scrollIntoView({ block: 'start' })
  editor.value.revealLines(location.lineStart, location.lineEnd)
  located.value = location
}

watch(
  [() => props.location, source, error, loading, editorReady, editor],
  applyLocation,
  { flush: 'post' },
)
watch(
  auth.generation,
  () => {
    if (!auth.isCurrent(props.identity)) {
      stop()
      editor.value?.clearHighlight()
      editorReady.value = false
      located.value = null
    }
  },
  { flush: 'sync' },
)
</script>

<template>
  <section
    ref="panel"
    class="surface result-source"
    aria-labelledby="source-title"
  >
    <div class="panel-heading">
      <h2 id="source-title">本次提交源码</h2>
      <span v-if="source" class="muted">
        {{ source.language_id === 'c' ? 'C' : 'Python 3' }} · 只读
      </span>
    </div>
    <RequestFeedback
      :loading="loading"
      loading-text="正在读取本次提交源码…"
      :error="error"
      retry-text="重试读取源码"
      @retry="reload"
    />
    <template v-if="source && display">
      <p v-if="display.hasStandaloneCR" class="result-note">
        单独的回车字符以 ␍ 显示，行号按 LF 换行计算。
      </p>
      <CodeEditor
        ref="editor"
        :model-value="display.text"
        :language="source.language_id"
        read-only
        @ready="editorReady = true"
      />
      <p v-if="located" class="result-note" role="status">
        已定位：第 {{ located.lineStart
        }}<template v-if="located.lineEnd !== located.lineStart"
          >–{{ located.lineEnd }}</template
        >
        行
      </p>
    </template>
  </section>
</template>

<style scoped>
.result-source {
  scroll-margin-top: 24px;
}
</style>
