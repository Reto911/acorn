<script setup lang="ts">
import {
  computed,
  defineAsyncComponent,
  onMounted,
  onScopeDispose,
  ref,
} from 'vue'
import {
  ElAlert,
  ElButton,
  ElMessage,
  ElMessageBox,
  ElOption,
  ElSelect,
} from 'element-plus'
import { useRouter } from 'vue-router'
import type { Problem } from '../api/types'
import { useDraft } from '../composables/useDraft'
import { useSubmissionRequest } from '../composables/useSubmissionRequest'
import 'element-plus/es/components/alert/style/css'
import 'element-plus/es/components/button/style/css'
import 'element-plus/es/components/message/style/css'
import 'element-plus/es/components/message-box/style/css'
import 'element-plus/es/components/select/style/css'
import 'element-plus/es/components/option/style/css'

const props = defineProps<{ problem: Problem; submissionDisabled: boolean }>()
const router = useRouter()
const {
  source,
  languageId,
  saved,
  storageWarning,
  versionNotice,
  selectLanguage,
  save,
  flush,
  clearConfirmed,
  dismissVersionNotice,
} = useDraft(() => props.problem)
const { pending, sending, start } = useSubmissionRequest()
const CodeEditor = defineAsyncComponent(() => import('./CodeEditor.vue'))
const confirming = ref(false)
let active = true

const limits = computed(() =>
  props.problem.languages.find(
    (item) => item.language_id === languageId.value,
  )!,
)
const sourceMaxBytes = Number(import.meta.env.VITE_SOURCE_MAX_BYTES ?? 65536)
const validLimit = Number.isSafeInteger(sourceMaxBytes) && sourceMaxBytes > 0
const sourceBytes = computed(
  () => new TextEncoder().encode(source.value).byteLength,
)
const sourceError = computed(() => {
  if (!validLimit) return '源码字节上限配置无效，请联系维护者。'
  if (sourceBytes.value > sourceMaxBytes)
    return '源码超过 UTF-8 字节上限，请缩减后再提交。'
  return ''
})
const cannotSubmit = computed(
  () =>
    props.submissionDisabled ||
    !!pending.value ||
    sending.value ||
    confirming.value ||
    source.value.length === 0 ||
    !!sourceError.value,
)

function saveWithFeedback() {
  if (save()) ElMessage.success('草稿已保存到本地')
  else ElMessage.error('草稿未能保存，请查看本地存储提示。')
}

function saveKey(event: KeyboardEvent) {
  if (
    (event.ctrlKey || event.metaKey) &&
    !event.altKey &&
    event.key.toLowerCase() === 's'
  ) {
    event.preventDefault()
    event.stopPropagation()
    if (!event.repeat) saveWithFeedback()
  }
}

async function clearDraft() {
  const language = languageId.value
  confirming.value = true
  try {
    await ElMessageBox.confirm(
      '只清空当前题目、当前语言的本地草稿，其他语言和待完成提交请求会保留。',
      '清空本语言草稿？',
      {
        type: 'warning',
        confirmButtonText: '确认清空',
        cancelButtonText: '保留草稿',
        distinguishCancelAndClose: true,
      },
    )
    if (active && languageId.value === language) {
      if (clearConfirmed()) ElMessage.success('本语言草稿已清空')
      else ElMessage.error('未能清除本地草稿，请查看存储提示。')
    }
  } catch (cause) {
    if (cause !== 'cancel' && cause !== 'close') throw cause
  } finally {
    confirming.value = false
  }
}

async function submit() {
  if (cannotSubmit.value) return
  save()
  const result = await start({
    problem_id: props.problem.problem_id,
    language_id: languageId.value,
    source: source.value,
  })
  if (result && active)
    await router.push({
      name: 'result',
      params: { submissionId: result.submission_id },
    })
}

onMounted(() => {
  window.addEventListener('keydown', saveKey, true)
  window.addEventListener('pagehide', flush)
})
onScopeDispose(() => {
  active = false
  window.removeEventListener('keydown', saveKey, true)
  window.removeEventListener('pagehide', flush)
  if (confirming.value) ElMessageBox.close()
})
</script>

<template>
  <section class="surface editor-panel" aria-labelledby="editor-title">
    <div class="panel-heading">
      <h2 id="editor-title">代码编辑区</h2>
      <div class="editor-language">
        <label for="submission-language">语言</label>
        <ElSelect
          id="submission-language"
          :model-value="languageId"
          :disabled="confirming"
          @update:model-value="selectLanguage"
        >
          <ElOption
            v-for="language in problem.languages"
            :key="language.language_id"
            :value="language.language_id"
            :label="language.language_id === 'c' ? 'C' : 'Python 3'"
          />
        </ElSelect>
      </div>
    </div>
    <div class="language-limits">
      <h3>{{ languageId === 'c' ? 'C' : 'Python 3' }} · 单用例限额</h3>
      <div class="language-limit">
        <dl>
          <div>
            <dt>CPU 时间</dt>
            <dd>{{ limits.cpu_time_ms }} ms</dd>
          </div>
          <div>
            <dt>墙钟时间</dt>
            <dd>{{ limits.wall_time_ms }} ms</dd>
          </div>
          <div>
            <dt>内存</dt>
            <dd>{{ limits.memory_kib / 1024 }} MiB</dd>
          </div>
        </dl>
      </div>
      <p>提交时将使用创建时的最新发布版本。</p>
    </div>
    <div v-if="versionNotice || storageWarning" class="draft-notices">
      <ElAlert
        v-if="versionNotice"
        :title="versionNotice"
        type="info"
        show-icon
        @close="dismissVersionNotice"
      />
      <ElAlert
        v-if="storageWarning"
        :title="storageWarning"
        type="warning"
        :closable="false"
        show-icon
      />
    </div>
    <CodeEditor v-model="source" :language="languageId" />
    <div class="editor-actions">
      <p class="draft-status" role="status">
        {{
          saved ? '草稿已保存到本地' : source.length ? '草稿尚未保存' : '空草稿'
        }}
      </p>
      <div class="draft-actions">
        <ElButton :disabled="confirming" @click="saveWithFeedback"
          >保存草稿</ElButton
        >
        <ElButton :disabled="confirming" @click="clearDraft"
          >清空本语言草稿</ElButton
        >
        <span class="muted">Ctrl+S / Cmd+S 保存草稿</span>
      </div>
      <p class="source-size">
        UTF-8：{{ sourceBytes
        }}<template v-if="validLimit"> / {{ sourceMaxBytes }}</template> 字节
      </p>
      <p v-if="sourceError" class="source-error" role="alert">
        {{ sourceError }}
      </p>
      <p v-if="pending" class="submission-guidance">
        有待完成的提交请求，请先使用上方入口重试或放弃原请求；编辑不会改变原提交。
      </p>
      <p v-else-if="source.length === 0" class="muted">请输入源码后提交。</p>
      <ElButton
        type="primary"
        :loading="sending"
        :disabled="cannotSubmit"
        @click="submit"
        >提交代码</ElButton
      >
    </div>
  </section>
</template>
