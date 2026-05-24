resource "azurerm_dns_cname_record" "cname" {
  name                = var.app-name
  resource_group_name = "dns"
  zone_name           = var.root-domain
  ttl                 = 300
  target_resource_id  = azurerm_static_web_app.website.id

  lifecycle {
    prevent_destroy = true
  }

  depends_on = [
    azurerm_static_web_app.website
  ]
}

data "cloudflare_zones" "root_domain" {
  filter {
    account_id = var.cloudflare_account_id
    name       = var.root-domain
    match      = "all"
  }
}

resource "cloudflare_record" "cname" {
  zone_id = data.cloudflare_zones.root_domain.zones[0].id
  name    = var.app-name
  type    = "CNAME"
  ttl     = 300
  value   = azurerm_static_web_app.website.default_host_name
  proxied = false

  lifecycle {
    prevent_destroy = true
  }
}
