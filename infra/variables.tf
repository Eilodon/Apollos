variable "project_id" {
  description = "Google Cloud project id"
  type        = string
}

variable "region" {
  description = "Deployment region"
  type        = string
  default     = "us-central1"
}

variable "service_name" {
  description = "Cloud Run service name"
  type        = string
  default     = "visiongpt-aria-backend"
}

variable "container_image" {
  description = "Container image URL for backend"
  type        = string
}

variable "enable_firestore" {
  description = "Whether to create Firestore database"
  type        = bool
  default     = true
}
