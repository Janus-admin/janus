# CloudFront distribution in front of the Lightsail VM.
# Provides automatic HTTPS via the default *.cloudfront.net certificate.
# Origin is the public IP over HTTP (port 80); viewer connection is forced to HTTPS.

resource "aws_cloudfront_distribution" "velox" {
  enabled         = true
  is_ipv6_enabled = true
  comment         = "Velox dashboard + API (origin: Lightsail VM)"
  price_class     = "PriceClass_100" # US/EU only - cheapest

  # CloudFront rejects IP origins, so route via sslip.io (free wildcard DNS that
  # resolves a-b-c-d.sslip.io to a.b.c.d). No AWS hosted zone needed.
  origin {
    domain_name = "${replace(aws_lightsail_static_ip.velox.ip_address, ".", "-")}.sslip.io"
    origin_id   = "velox-lightsail"

    custom_origin_config {
      http_port              = 80
      https_port             = 443
      origin_protocol_policy = "http-only"
      origin_ssl_protocols   = ["TLSv1.2"]
      origin_read_timeout    = 60 # bump if streaming chat completions need longer
      origin_keepalive_timeout = 30
    }
  }

  default_cache_behavior {
    target_origin_id       = "velox-lightsail"
    viewer_protocol_policy = "redirect-to-https"
    allowed_methods        = ["GET", "HEAD", "OPTIONS", "PUT", "POST", "PATCH", "DELETE"]
    cached_methods         = ["GET", "HEAD"]
    compress               = true

    # AWS managed policies (cleaner than inline forwarded_values):
    # - CachingDisabled (4135ea2d-6df8-44a3-9df3-4b5a84be39ad): no caching at all
    # - AllViewer       (216adef6-5c7f-47e4-b989-5492eafa07d3): forward everything
    cache_policy_id          = "4135ea2d-6df8-44a3-9df3-4b5a84be39ad"
    origin_request_policy_id = "216adef6-5c7f-47e4-b989-5492eafa07d3"
  }

  viewer_certificate {
    cloudfront_default_certificate = true
  }

  restrictions {
    geo_restriction {
      restriction_type = "none"
    }
  }

  tags = {
    Project = var.project
  }
}

output "https_url" {
  description = "HTTPS dashboard URL (takes 5-15 min to propagate after first apply)"
  value       = "https://${aws_cloudfront_distribution.velox.domain_name}"
}
