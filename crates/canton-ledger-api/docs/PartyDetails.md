# PartyDetails

## Properties

Name | Type | Description | Notes
------------ | ------------- | ------------- | -------------
**party** | **String** | The stable unique identifier of a Daml party. Must be a valid PartyIdString (as described in ``value.proto``). Required | 
**is_local** | **bool** | true if party is hosted by the participant and the party shares the same identity provider as the user issuing the request. Optional | 
**local_metadata** | Option<[**models::ObjectMeta**](ObjectMeta.md)> |  | [optional]
**identity_provider_id** | **String** | The id of the ``Identity Provider`` Optional, if not set, there could be 3 options:  1. the party is managed by the default identity provider. 2. party is not hosted by the participant. 3. party is hosted by the participant, but is outside of the user's identity provider. | 

[[Back to Model list]](../README.md#documentation-for-models) [[Back to API list]](../README.md#documentation-for-api-endpoints) [[Back to README]](../README.md)


