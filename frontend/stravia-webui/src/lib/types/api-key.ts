import type { ApiKeySpend } from './money'

export interface RequestModelMapping {
  request_model: string
  target_model_id: string
}
export interface RequestModelConfig {
  enabled: boolean
  mappings: RequestModelMapping[]
}
export interface RequestModelsView {
  config: RequestModelConfig
  eligible_models: { id: string; model_id: string; display_name: string }[]
  capture: {
    active: boolean
    expires_at: string | null
    dropped_samples: number
    samples: {
      request_model: string
      protocol: string
      transport: string
      stream: boolean
      input_items: number
      tools: number
      count: number
      client?: { family: string; version: string | null; runtime_version: string | null; os: string | null; arch: string | null; suggested_http_shape: string | null; suggested_ws_shape: string | null }
      parameters?: { max_output_tokens: number | null; thinking_budget_tokens: number | null; thinking_level: string | null; thinking_enabled: boolean }
    }[]
  }
  suggestions: { request_model: string; target_model_id: string | null; candidates: string[] }[]
}

export interface ApiKey {
  id: string
  key: string
  name: string
  rpm_limit: number | null
  /** Decimal string in the current display currency; null means unlimited. */
  spend_limit?: string | null
  spend?: ApiKeySpend
  /** 剩余可消费金额（展示币种，已消费超出上限时为 "0"）；不限时为 null。 */
  spend_available?: string | null
  is_enabled: boolean
  mcp_access_enabled: boolean
  transparent_injection_enabled: boolean
  inject_web_search: boolean
  inject_media_understanding: boolean
  inject_media_generation: boolean
  expires_at?: string | null
  created_at: string
  updated_at: string
  model_ids: string[]
}

export interface CreateApiKey {
  key?: string
  name: string
  rpm_limit?: number | null
  spend_limit?: string | null
  mcp_access_enabled: boolean
  transparent_injection_enabled: boolean
  inject_web_search: boolean
  inject_media_understanding: boolean
  inject_media_generation: boolean
  expires_at?: string
  model_ids: string[]
}

export interface UpdateApiKey {
  key?: string
  name?: string
  rpm_limit?: number | null
  spend_limit?: string | null
  is_enabled?: boolean
  mcp_access_enabled?: boolean
  transparent_injection_enabled?: boolean
  inject_web_search?: boolean
  inject_media_understanding?: boolean
  inject_media_generation?: boolean
  expires_at?: string
  model_ids?: string[]
}
