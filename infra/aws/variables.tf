variable "aws_region" {
  description = "AWS region for Lightsail + ECR"
  type        = string
  default     = "eu-north-1"
}

variable "project" {
  description = "Project name; used as prefix for all resources"
  type        = string
  default     = "janus"
}

# ── Lightsail VM sizing ──────────────────────────────────────────────────────
variable "bundle_id" {
  description = "Lightsail instance plan. small_3_0 = 2GB/1vCPU/60GB/$10"
  type        = string
  default     = "small_3_0"
}

variable "availability_zone" {
  description = "AZ inside aws_region. Must end with a/b/c."
  type        = string
  default     = "eu-north-1a"
}

# ── App secrets pulled from local .env, never committed ──────────────────────
# These are passed into the VM via user_data and become container env vars.
variable "openai_api_key" {
  type      = string
  sensitive = true
  default   = ""
}
variable "anthropic_api_key" {
  type      = string
  sensitive = true
  default   = ""
}
variable "gemini_api_key" {
  type      = string
  sensitive = true
  default   = ""
}
variable "groq_api_key" {
  type      = string
  sensitive = true
  default   = ""
}
variable "deepseek_api_key" {
  type      = string
  sensitive = true
  default   = ""
}

# AWS creds used INSIDE the VM (for Bedrock provider + ECR pull).
# Lightsail VMs do not support IAM roles, so credentials must be embedded.
variable "vm_aws_access_key_id" {
  type      = string
  sensitive = true
}
variable "vm_aws_secret_access_key" {
  type      = string
  sensitive = true
}
