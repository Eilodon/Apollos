output "cloud_run_url" {
  value       = google_cloud_run_v2_service.aria_backend.uri
  description = "Public URL for ARIA backend"
}

output "service_account_email" {
  value       = google_service_account.aria_backend.email
  description = "Runtime service account"
}
