data "aws_caller_identity" "current" {}

locals {
  ecr_image_uri = "${data.aws_caller_identity.current.account_id}.dkr.ecr.${var.aws_region}.amazonaws.com/${var.project}:latest"
}

# ── ECR repository for the Janus image ───────────────────────────────────────
resource "aws_ecr_repository" "janus" {
  name                 = var.project
  image_tag_mutability = "MUTABLE"
  force_delete         = true

  image_scanning_configuration {
    scan_on_push = false
  }
}

resource "aws_ecr_lifecycle_policy" "janus" {
  repository = aws_ecr_repository.janus.name
  policy = jsonencode({
    rules = [{
      rulePriority = 1
      description  = "Keep last 5 images"
      selection = {
        tagStatus   = "any"
        countType   = "imageCountMoreThan"
        countNumber = 5
      }
      action = { type = "expire" }
    }]
  })
}

# ── Secrets generated at apply-time ──────────────────────────────────────────
resource "random_password" "db" {
  length  = 32
  special = false
}

resource "random_password" "jwt" {
  length  = 48
  special = false
}

# AES-256-GCM key must be exactly 32 bytes; we generate 32 raw bytes and base64 encode.
resource "random_bytes" "encryption_key" {
  length = 32
}

# ── Lightsail static IP ──────────────────────────────────────────────────────
resource "aws_lightsail_static_ip" "janus" {
  name = "${var.project}-ip"
}

resource "aws_lightsail_static_ip_attachment" "janus" {
  static_ip_name = aws_lightsail_static_ip.janus.id
  instance_name  = aws_lightsail_instance.janus.name
}

# ── Lightsail instance ───────────────────────────────────────────────────────
resource "aws_lightsail_instance" "janus" {
  name              = "${var.project}-app"
  availability_zone = var.availability_zone
  blueprint_id      = "ubuntu_22_04"
  bundle_id         = var.bundle_id

  user_data = templatefile("${path.module}/user_data.sh.tpl", {
    aws_region            = var.aws_region
    aws_access_key_id     = var.vm_aws_access_key_id
    aws_secret_access_key = var.vm_aws_secret_access_key
    ecr_account           = data.aws_caller_identity.current.account_id
    ecr_image             = local.ecr_image_uri
    db_password           = random_password.db.result
    jwt_secret            = random_password.jwt.result
    encryption_key        = random_bytes.encryption_key.base64
    openai_api_key        = var.openai_api_key
    anthropic_api_key     = var.anthropic_api_key
    gemini_api_key        = var.gemini_api_key
    groq_api_key          = var.groq_api_key
    deepseek_api_key      = var.deepseek_api_key
  })

  tags = {
    Project = var.project
  }

  # Don't recreate the instance just because the user_data template changed.
  # The current VM was bootstrapped by hand; rebooting it would lose the working state.
  # To force a fresh VM, taint explicitly: `terraform taint aws_lightsail_instance.janus`.
  lifecycle {
    ignore_changes = [user_data]
  }
}

# NOTE: Firewall ports are NOT managed by Terraform.
# The aws_lightsail_instance_public_ports resource has known bugs where:
#   - it hangs on destroy for ~30 min,
#   - it always detects ipv6/cidr drift and forces replacement on every plan.
# Instead we open ports once at bootstrap via:
#   aws lightsail put-instance-public-ports --instance-name janus-app \
#     --port-infos '[{"fromPort":22,"toPort":22,"protocol":"tcp"},{"fromPort":80,"toPort":80,"protocol":"tcp"}]'
