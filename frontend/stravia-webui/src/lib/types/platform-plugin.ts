export interface PlatformPluginTool {
  name: string
  description: string
  parameters: unknown
  parallel_safe: boolean
}

export interface PlatformPluginManifest {
  id: string
  name: string
  version: string
  api_version: number
  config_schema?: unknown
  hooks: string[]
  tools: PlatformPluginTool[]
}

export interface PlatformPluginSummary {
  manifest: PlatformPluginManifest
  sha256: string
  enabled: boolean
  config: unknown
  error?: string | null
}

export interface PlatformPluginSettings {
  enabled?: boolean
  config?: unknown
}
