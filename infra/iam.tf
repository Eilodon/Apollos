resource "google_service_account" "aria_backend" {
  project      = var.project_id
  account_id   = "visiongpt-aria-backend"
  display_name = "VisionGPT ARIA backend runtime"
}

resource "google_project_iam_member" "run_invoker" {
  project = var.project_id
  role    = "roles/run.invoker"
  member  = "serviceAccount:${google_service_account.aria_backend.email}"
}

resource "google_project_iam_member" "vertex_user" {
  project = var.project_id
  role    = "roles/aiplatform.user"
  member  = "serviceAccount:${google_service_account.aria_backend.email}"
}

resource "google_project_iam_member" "firestore_user" {
  count   = var.enable_firestore ? 1 : 0
  project = var.project_id
  role    = "roles/datastore.user"
  member  = "serviceAccount:${google_service_account.aria_backend.email}"
}
