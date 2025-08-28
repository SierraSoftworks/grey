# Azure Managed Service Identities
If you're using [Microsoft Azure](https://azure.microsoft.com) and relying on
[Azure AD](https://azure.microsoft.com/en-us/products/active-directory) for
service to service (S2S) authentication then there's a good chance you'll
find it useful to leverage Managed Service Identities within Grey.

[Managed Service Identities](https://learn.microsoft.com/en-us/azure/active-directory/managed-identities-azure-resources/overview)
(MSIs) are an extremely useful means of authenticating a service without the
need to manage secrets. You can use MSIs when running Grey on an Azure VM,
Container, Kubernetes cluster, or AppService plan by leveraging the
[`!Script`](../targets/script.md) execution target as shown below.

## Helper Function
The following is a helper function that can help you retrieve an access token
for the provided `resource` within your `!Script` target.

```js{1-24}
async function getAccessToken(args = {}) {
    args = Object.assign({}, {
        resource: "https://management.azure.com/",
        api_version: "2021-12-13"
    }, args)

    const queryString = Object.keys(args).map(k => `${k}=${encodeUrlParameter(args[k])}`).join("&")

    const resp = await fetch(`http://169.254.169.254/metadata/identity/oauth2/token?${queryString}`, {
        headers: {
            Metadata: "true"
        }
    })

    if (!resp.ok) {
        throw new Error(`${resp.status} ${resp.statusText}: ${await resp.text()}`)
    }

    const token = await resp.json()

    // NOTE: You can find more details about the properties available here at:
    // https://learn.microsoft.com/en-us/azure/active-directory/managed-identities-azure-resources/how-to-use-vm-token#get-a-token-using-http
    return token.access_token
}

// NOTE: The following is an example of using this helper function

const accessToken = await getAccessToken({
    resource: "https://myapp.example.com/"
})

const resp = await fetch("https://myapp.example.com/api/v1/data", {
    headers: {
        Authorization: `Bearer ${accessToken}`
    }
})

setOutput('http.status_code', resp.status)

if (resp.ok) {
    // Do any content assertions you wish to do here
}
```
