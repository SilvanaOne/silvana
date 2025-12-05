# RevokeUserRightsRequest

## Properties

Name | Type | Description | Notes
------------ | ------------- | ------------- | -------------
**user_id** | **String** | The user from whom to revoke rights. Required | 
**rights** | Option<[**Vec<models::Right>**](Right.md)> | The rights to revoke. Optional | [optional]
**identity_provider_id** | **String** | The id of the ``Identity Provider`` Optional, if not set, assume the user is managed by the default identity provider. | 

[[Back to Model list]](../README.md#documentation-for-models) [[Back to API list]](../README.md#documentation-for-api-endpoints) [[Back to README]](../README.md)


