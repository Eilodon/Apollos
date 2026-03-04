provider "google" {
  project = var.project_id
  region  = var.region
}

locals {
  labels = {
    app       = "visiongpt"
    component = "aria-backend"
    managedBy = "terraform"
  }
}
