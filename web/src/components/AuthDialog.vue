<script setup lang="ts">
import { computed, nextTick, reactive, ref, watch } from 'vue'
import {
  ElAlert,
  ElButton,
  ElCheckbox,
  ElDialog,
  ElForm,
  ElFormItem,
  ElInput,
  type FormInstance,
  type FormItemRule,
  type FormRules,
} from 'element-plus'
import { RequestError } from '../api/http'
import {
  normalizeDisplayName,
  normalizeUsername,
  validateDisplayName,
  validatePassword,
  validateUsername,
} from '../auth/validation'
import { useAuth } from '../composables/useAuth'
import 'element-plus/es/components/alert/style/css'
import 'element-plus/es/components/button/style/css'
import 'element-plus/es/components/checkbox/style/css'
import 'element-plus/es/components/dialog/style/css'
import 'element-plus/es/components/form/style/css'
import 'element-plus/es/components/form-item/style/css'
import 'element-plus/es/components/input/style/css'

const {
  dialogOpen,
  closeDialog,
  login,
  register,
  busy,
  formError,
  clearFormError,
} = useAuth()
const mode = ref<'login' | 'register'>('login')
const form = reactive({ username: '', password: '', display_name: '' })
const customDisplayName = ref(false)
const formRef = ref<FormInstance>()
const notice = ref('')

/** Element Plus owns validation timing and field error presentation. */
function validator(check: (value: string) => string | undefined): FormItemRule {
  return {
    trigger: 'blur',
    validator: (_rule, value: string, callback) => {
      const error = check(value)
      callback(error ? new Error(error) : undefined)
    },
  }
}

const rules = computed<FormRules>(() => ({
  username: [validator(validateUsername)],
  password: [validator(validatePassword)],
  display_name:
    mode.value === 'register' && customDisplayName.value
      ? [validator(validateDisplayName)]
      : [],
}))

/** Authentication feedback never echoes a request value or server response body. */
const errorText = computed(() => {
  const error = formError.value
  if (!error) return ''
  if (error instanceof RequestError) {
    switch (error.code) {
      case 'InvalidCredentials':
        return '用户名或密码不正确，请检查后重试。'
      case 'UsernameTaken':
        return '该用户名已被使用，请更换用户名。'
      case 'InvalidRequest':
        return '输入内容不符合要求，请检查后重试。'
      case 'RequestOriginRejected':
        return '请求来源不被允许，请联系维护者。'
    }
    if (error.kind === 'network') return '未能连接服务，请稍后重试。'
    if (error.kind === 'timeout') return '请求等待超时，请稍后重试。'
  }
  return '本次操作未能完成，请稍后重试。'
})

async function switchMode() {
  mode.value = mode.value === 'login' ? 'register' : 'login'
  form.password = ''
  clearFormError()
  notice.value = ''
  await nextTick()
  formRef.value?.clearValidate()
}

async function submit() {
  if (busy.value) return
  const valid = await formRef.value?.validate().catch(() => false)
  if (!valid) return
  notice.value = ''
  const username = normalizeUsername(form.username)
  if (mode.value === 'login') {
    if (await login({ username, password: form.password })) {
      form.password = ''
      closeDialog()
    }
    return
  }
  const succeeded = await register({
    username,
    password: form.password,
    ...(customDisplayName.value
      ? { display_name: normalizeDisplayName(form.display_name) }
      : {}),
  })
  if (!succeeded) return
  mode.value = 'login'
  form.username = username
  form.password = ''
  notice.value = '注册成功，请使用用户名和密码手动登录。'
  await nextTick()
  formRef.value?.clearValidate()
}

watch(dialogOpen, (open) => {
  if (!open) {
    form.password = ''
    mode.value = 'login'
    notice.value = ''
    formRef.value?.clearValidate()
  }
})

watch(customDisplayName, () => formRef.value?.clearValidate('display_name'))
</script>

<template>
  <ElDialog
    :model-value="dialogOpen"
    :title="mode === 'login' ? '登录 Acorn' : '注册 Acorn 账号'"
    width="min(480px, calc(100% - 32px))"
    :close-on-click-modal="!busy"
    :close-on-press-escape="!busy"
    :show-close="!busy"
    @update:model-value="!$event && closeDialog()"
  >
    <p class="auth-description">登录后可提交代码、查看个人历史及代码审查。</p>
    <ElAlert
      v-if="notice"
      :title="notice"
      type="success"
      :closable="false"
      show-icon
    />
    <ElAlert
      v-if="errorText"
      :title="errorText"
      type="error"
      :closable="false"
      show-icon
    />
    <ElForm
      ref="formRef"
      class="auth-form"
      :model="form"
      :rules="rules"
      label-position="top"
      :disabled="busy"
      @submit.prevent="submit"
    >
      <ElFormItem label="用户名" prop="username">
        <ElInput
          v-model="form.username"
          autocomplete="username"
          placeholder="3–32 个英文字母、数字或下划线"
        />
      </ElFormItem>
      <template v-if="mode === 'register'">
        <ElCheckbox v-model="customDisplayName"
          >设置显示名（默认使用用户名）</ElCheckbox
        >
        <ElFormItem v-if="customDisplayName" label="显示名" prop="display_name">
          <ElInput
            v-model="form.display_name"
            autocomplete="nickname"
            placeholder="1–64 个 Unicode 字符"
          />
        </ElFormItem>
      </template>
      <ElFormItem label="密码" prop="password">
        <ElInput
          v-model="form.password"
          type="password"
          show-password
          :autocomplete="mode === 'login' ? 'current-password' : 'new-password'"
          placeholder="8–128 个 Unicode 字符，不含空白"
        />
      </ElFormItem>
      <p class="auth-password-note">
        密码最多 512 UTF-8 字节，无需特定字符组合。
      </p>
      <div class="auth-actions">
        <ElButton :disabled="busy" @click="closeDialog()">取消</ElButton>
        <ElButton :disabled="busy" @click="switchMode">
          {{ mode === 'login' ? '注册账号' : '返回登录' }}
        </ElButton>
        <ElButton type="primary" native-type="submit" :loading="busy">
          {{ mode === 'login' ? '登录' : '注册' }}
        </ElButton>
      </div>
    </ElForm>
  </ElDialog>
</template>

<style scoped>
.auth-description,
.auth-password-note {
  color: #68756c;
  line-height: 1.8;
}

.auth-description {
  margin-bottom: 16px;
}

.auth-form {
  margin-top: 20px;
}

.auth-password-note {
  font-size: 12px;
}

.auth-actions {
  display: flex;
  flex-wrap: wrap;
  justify-content: flex-end;
  gap: 8px;
  margin-top: 20px;
}

.auth-actions .el-button + .el-button {
  margin-left: 0;
}
</style>
