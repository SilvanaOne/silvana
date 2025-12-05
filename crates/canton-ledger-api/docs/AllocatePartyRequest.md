# AllocatePartyRequest

## Properties

Name | Type | Description | Notes
------------ | ------------- | ------------- | -------------
**party_id_hint** | **String** | A hint to the participant which party ID to allocate. It can be ignored. Must be a valid PartyIdString (as described in ``value.proto``). Optional | 
**local_metadata** | Option<[**models::ObjectMeta**](ObjectMeta.md)> |  | [optional]
**identity_provider_id** | **String** | The id of the ``Identity Provider`` Optional, if not set, assume the party is managed by the default identity provider or party is not hosted by the participant. | 

[[Back to Model list]](../README.md#documentation-for-models) [[Back to API list]](../README.md#documentation-for-api-endpoints) [[Back to README]](../README.md)


