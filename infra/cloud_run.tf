resource "google_cloud_run_v2_service" "aria_backend" {
  name     = var.service_name
  location = var.region

  ingress = "INGRESS_TRAFFIC_ALL"

  template {
    service_account = google_service_account.aria_backend.email

    containers {
      image = var.container_image

      env {
        name  = "GOOGLE_CLOUD_PROJECT"
        value = var.project_id
      }

      env {
        name  = "USE_FIRESTORE"
        value = var.enable_firestore ? "1" : "0"
      }

      env {
        name  = "SINGLE_INSTANCE_ONLY"
        value = "1"
      }

      env {
        name  = "WS_AUTH_MODE"
        value = "oidc_broker"
      }

      resources {
        limits = {
          cpu    = "2"
          memory = "2Gi"
        }
      }
    }

    scaling {
      min_instance_count = 0
      max_instance_count = 1
    }
  }

  labels = local.labels
}

resource "google_cloud_run_v2_service_iam_member" "public_invoker" {
  project  = var.project_id
  location = var.region
  name     = google_cloud_run_v2_service.aria_backend.name
  role     = "roles/run.invoker"
  member   = "allUsers"
}
