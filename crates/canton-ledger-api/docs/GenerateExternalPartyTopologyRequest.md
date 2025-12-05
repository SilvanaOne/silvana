# GenerateExternalPartyTopologyRequest

## Properties

Name | Type | Description | Notes
------------ | ------------- | ------------- | -------------
**synchronizer** | **String** | TODO(#27670) support synchronizer aliases Required: synchronizer-id for which we are building this request. | 
**party_hint** | **String** | Required: the actual party id will be constructed from this hint and a fingerprint of the public key | 
**public_key** | Option<[**models::SigningPublicKey**](SigningPublicKey.md)> |  | [optional]
**local_participant_observation_only** | **bool** | Optional: if true, then the local participant will only be observing, not confirming. Default false. | 
**other_confirming_participant_uids** | Option<**Vec<String>**> | Optional: other participant ids which should be confirming for this party | [optional]
**confirmation_threshold** | **i32** | Optional: Confirmation threshold >= 1 for the party. Defaults to all available confirmers (or if set to 0). | 
**observing_participant_uids** | Option<**Vec<String>**> | Optional: other observing participant ids for this party | [optional]

[[Back to Model list]](../README.md#documentation-for-models) [[Back to API list]](../README.md#documentation-for-api-endpoints) [[Back to README]](../README.md)


