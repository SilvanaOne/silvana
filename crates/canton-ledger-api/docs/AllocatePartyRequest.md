# AllocatePartyRequest

## Properties

Name | Type | Description | Notes
------------ | ------------- | ------------- | -------------
**party_id_hint** | **String** | A hint to the participant which party ID to allocate. It can be ignored. Must be a valid PartyIdString (as described in ``value.proto``). Optional | 
**local_metadata** | Option<[**models::ObjectMeta**](ObjectMeta.md)> |  | [optional]
**identity_provider_id** | **String** | The id of the ``Identity Provider`` Optional, if not set, assume the party is managed by the default identity provider or party is not hosted by the participant. | 
**synchronizer_id** | **String** | The synchronizer, on which the party should be allocated. For backwards compatibility, this field may be omitted, if the participant is connected to only one synchronizer. Otherwise a synchronizer must be specified. Optional | 
**user_id** | **String** | The user who will get the act_as rights to the newly allocated party. If set to an empty string (the default), no user will get rights to the party. Optional | 

[[Back to Model list]](../README.md#documentation-for-models) [[Back to API list]](../README.md#documentation-for-api-endpoints) [[Back to README]](../README.md)


