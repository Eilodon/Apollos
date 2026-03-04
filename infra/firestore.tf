resource "google_firestore_database" "default" {
  count       = var.enable_firestore ? 1 : 0
  project     = var.project_id
  name        = "(default)"
  location_id = var.region
  type        = "FIRESTORE_NATIVE"
}
