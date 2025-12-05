# User

## Properties

Name | Type | Description | Notes
------------ | ------------- | ------------- | -------------
**id** | **String** | The user identifier, which must be a non-empty string of at most 128 characters that are either alphanumeric ASCII characters or one of the symbols \"@^$.!`-#+'~_|:\". Required | 
**primary_party** | **String** | The primary party as which this user reads and acts by default on the ledger *provided* it has the corresponding ``CanReadAs(primary_party)`` or ``CanActAs(primary_party)`` rights. Ledger API clients SHOULD set this field to a non-empty value for all users to enable the users to act on the ledger using their own Daml party. Users for participant administrators MAY have an associated primary party. Optional, Modifiable | 
**is_deactivated** | **bool** | When set, then the user is denied all access to the Ledger API. Otherwise, the user has access to the Ledger API as per the user's rights. Optional, Modifiable | 
**metadata** | Option<[**models::ObjectMeta**](ObjectMeta.md)> |  | [optional]
**identity_provider_id** | **String** | The ID of the identity provider configured by ``Identity Provider Config`` Optional, if not set, assume the user is managed by the default identity provider. | 

[[Back to Model list]](../README.md#documentation-for-models) [[Back to API list]](../README.md#documentation-for-api-endpoints) [[Back to README]](../README.md)


