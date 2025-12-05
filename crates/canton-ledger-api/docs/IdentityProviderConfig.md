# IdentityProviderConfig

## Properties

Name | Type | Description | Notes
------------ | ------------- | ------------- | -------------
**identity_provider_id** | **String** | The identity provider identifier Must be a valid LedgerString (as describe in ``value.proto``). Required | 
**is_deactivated** | **bool** | When set, the callers using JWT tokens issued by this identity provider are denied all access to the Ledger API. Optional, Modifiable | 
**issuer** | **String** | Specifies the issuer of the JWT token. The issuer value is a case sensitive URL using the https scheme that contains scheme, host, and optionally, port number and path components and no query or fragment components. Required Modifiable | 
**jwks_url** | **String** | The JWKS (JSON Web Key Set) URL. The Ledger API uses JWKs (JSON Web Keys) from the provided URL to verify that the JWT has been signed with the loaded JWK. Only RS256 (RSA Signature with SHA-256) signing algorithm is supported. Required Modifiable | 
**audience** | **String** | Specifies the audience of the JWT token. When set, the callers using JWT tokens issued by this identity provider are allowed to get an access only if the \"aud\" claim includes the string specified here Optional, Modifiable | 

[[Back to Model list]](../README.md#documentation-for-models) [[Back to API list]](../README.md#documentation-for-api-endpoints) [[Back to README]](../README.md)


