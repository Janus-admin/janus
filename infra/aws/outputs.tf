output "ecr_repository_url" {
  description = "Push images here: docker push <this>:latest"
  value       = aws_ecr_repository.janus.repository_url
}

output "ecr_image_uri" {
  description = "Full image URI used by the VM"
  value       = "${aws_ecr_repository.janus.repository_url}:latest"
}

output "public_ip" {
  description = "Lightsail static IP"
  value       = aws_lightsail_static_ip.janus.ip_address
}

output "url" {
  description = "Open this in a browser once cloud-init finishes"
  value       = "http://${aws_lightsail_static_ip.janus.ip_address}"
}

output "ssh_cmd" {
  description = "SSH into the VM (download the default key from the Lightsail console first)"
  value       = "ssh -i ~/.ssh/LightsailDefaultKey-${var.aws_region}.pem ubuntu@${aws_lightsail_static_ip.janus.ip_address}"
}

output "tail_logs_cmd" {
  description = "After SSH-ing in, watch the app logs"
  value       = "sudo docker compose -f /opt/janus/docker-compose.yml logs -f app"
}
