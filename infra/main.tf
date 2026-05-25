variable "app-name" {
  description = "The name of the static web app to create."
  type        = string
  default     = "grey"
}

variable "root-domain" {
  description = "The domain name which will be used for this website."
  type        = string
  default     = "sierrasoftworks.com"
}

variable "tags" {
  description = "The tags which should apply to the resource."
  type        = map(string)
  default     = {}
}

variable "cloudflare_account_id" {
  description = "The Cloudflare account ID to use when discovering the DNS zone."
  type        = string
}
