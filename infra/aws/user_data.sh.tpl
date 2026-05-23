#!/bin/sh
# cloud-init script for the Velox Lightsail VM.
# Lightsail prepends its own `#!/bin/sh` SSH-CA setup, so this body must be dash-compatible.
set -eux

# Log everything to /var/log/velox-bootstrap.log
exec >>/var/log/velox-bootstrap.log 2>&1

export DEBIAN_FRONTEND=noninteractive

# 1) Install Docker engine + compose plugin (Ubuntu 22.04 has these in the universe repo).
apt-get update
apt-get install -y ca-certificates curl gnupg unzip

install -m 0755 -d /etc/apt/keyrings
curl -fsSL https://download.docker.com/linux/ubuntu/gpg | gpg --dearmor -o /etc/apt/keyrings/docker.gpg
chmod a+r /etc/apt/keyrings/docker.gpg

echo "deb [arch=$(dpkg --print-architecture) signed-by=/etc/apt/keyrings/docker.gpg] https://download.docker.com/linux/ubuntu jammy stable" \
  > /etc/apt/sources.list.d/docker.list
apt-get update
apt-get install -y docker-ce docker-ce-cli containerd.io docker-buildx-plugin docker-compose-plugin
systemctl enable --now docker

# 2) Install AWS CLI v2 (for ECR login).
curl -sSL "https://awscli.amazonaws.com/awscli-exe-linux-x86_64.zip" -o /tmp/awscliv2.zip
unzip -q /tmp/awscliv2.zip -d /tmp
/tmp/aws/install
rm -rf /tmp/aws /tmp/awscliv2.zip

# 3) Stash AWS creds for the ubuntu user (the app reads them from env, not from this file).
mkdir -p /root/.aws
cat >/root/.aws/credentials <<EOF
[default]
aws_access_key_id=${aws_access_key_id}
aws_secret_access_key=${aws_secret_access_key}
EOF
cat >/root/.aws/config <<EOF
[default]
region=${aws_region}
EOF
chmod 600 /root/.aws/credentials

# 4) Lay down the app workspace.
mkdir -p /opt/velox
cd /opt/velox

cat >.env <<EOF
ECR_IMAGE=${ecr_image}
DB_PASSWORD=${db_password}
JWT_SECRET=${jwt_secret}
ENCRYPTION_KEY=${encryption_key}
AWS_ACCESS_KEY_ID=${aws_access_key_id}
AWS_SECRET_ACCESS_KEY=${aws_secret_access_key}
AWS_REGION=${aws_region}
OPENAI_API_KEY=${openai_api_key}
ANTHROPIC_API_KEY=${anthropic_api_key}
GEMINI_API_KEY=${gemini_api_key}
GROQ_API_KEY=${groq_api_key}
DEEPSEEK_API_KEY=${deepseek_api_key}
EOF
chmod 600 .env

cat >docker-compose.yml <<'YAML'
services:
  db:
    image: postgres:16-alpine
    restart: unless-stopped
    environment:
      POSTGRES_DB: velox
      POSTGRES_USER: velox
      POSTGRES_PASSWORD: $${DB_PASSWORD}
    volumes:
      - pgdata:/var/lib/postgresql/data
    healthcheck:
      test: ["CMD-SHELL", "pg_isready -U velox"]
      interval: 5s
      timeout: 5s
      retries: 10

  app:
    image: $${ECR_IMAGE}
    restart: unless-stopped
    ports:
      - "80:8080"
    environment:
      DATABASE_URL: postgres://velox:$${DB_PASSWORD}@db:5432/velox
      JWT_SECRET: $${JWT_SECRET}
      ENCRYPTION_KEY: $${ENCRYPTION_KEY}
      OPENAI_API_KEY: $${OPENAI_API_KEY}
      ANTHROPIC_API_KEY: $${ANTHROPIC_API_KEY}
      GEMINI_API_KEY: $${GEMINI_API_KEY}
      GROQ_API_KEY: $${GROQ_API_KEY}
      DEEPSEEK_API_KEY: $${DEEPSEEK_API_KEY}
      AWS_ACCESS_KEY_ID: $${AWS_ACCESS_KEY_ID}
      AWS_SECRET_ACCESS_KEY: $${AWS_SECRET_ACCESS_KEY}
      AWS_REGION: $${AWS_REGION}
      RUST_LOG: info,tower_http=debug
    depends_on:
      db:
        condition: service_healthy

volumes:
  pgdata:
YAML

# 5) Authenticate Docker to ECR and pull the image. Retries because the image may
#    not have been pushed yet on the first boot of a fresh terraform apply.
aws ecr get-login-password --region "${aws_region}" \
  | docker login --username AWS --password-stdin "${ecr_account}.dkr.ecr.${aws_region}.amazonaws.com"

# 6) Bring it up. compose will keep restarting until the image is pullable.
docker compose up -d || true

# 7) Background watcher: poll for the image every 30s and start once available.
cat >/usr/local/bin/velox-wait-for-image.sh <<'WATCHER'
#!/bin/bash
set -eux
cd /opt/velox
for i in $(seq 1 60); do
  aws ecr get-login-password --region AWS_REGION_PLACEHOLDER \
    | docker login --username AWS --password-stdin ECR_HOST_PLACEHOLDER || true
  if docker compose pull app; then
    docker compose up -d
    exit 0
  fi
  sleep 30
done
WATCHER
sed -i "s|AWS_REGION_PLACEHOLDER|${aws_region}|g" /usr/local/bin/velox-wait-for-image.sh
sed -i "s|ECR_HOST_PLACEHOLDER|${ecr_account}.dkr.ecr.${aws_region}.amazonaws.com|g" /usr/local/bin/velox-wait-for-image.sh
chmod +x /usr/local/bin/velox-wait-for-image.sh
nohup /usr/local/bin/velox-wait-for-image.sh >/var/log/velox-wait.log 2>&1 &

echo "velox bootstrap complete"
